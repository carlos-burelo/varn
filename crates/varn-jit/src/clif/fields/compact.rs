//! Compact class-field access (`FieldAccess::Compact`): the field sits at a
//! byte offset of the instance payload the compiler baked, in the
//! representation its `TypeLayout` gives. [`load_compact`] and
//! [`store_compact`] are the one lowering of that access, used by the
//! bytecode lowering (the arms below) and by the lowering from typed SSA.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags, Value};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::FunctionBuilder;
use varn_types::layout::{ScalarRepr, TypeLayout, COMPACT_REF_NULL};
use varn_types::vm_value::{KIND_HEAP, KIND_NULL};

use super::super::alloc::AllocCtx;
use super::super::emit::{self, box_or_pass, call_helper_void, unbox_f64_coerce, use_boxed};
use super::super::kinds::K;
use super::FldCtx;
use crate::JitHelpers;

/// What a compact access needs besides its operands.
pub(crate) struct FieldIo<'a> {
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: Value,
}

/// Read the compact field at `offset` of boxed receiver `obj` as a boxed
/// `VmValue`. A receiver that is not a compact instance (an object, a
/// record, `null`) takes the `get_fixed_field` helper by `slot`.
pub(crate) fn load_compact(
    b: &mut FunctionBuilder,
    io: &FieldIo,
    obj: Value,
    offset: u32,
    tag: Option<varn_core::RuntimeKind>,
    slot: usize,
) -> Value {
    let slow = b.create_block();
    let cont = b.create_block();
    b.append_block_param(cont, types::I128);

    let data_base = emit::emit_object_data_base(
        b,
        io.exec_ctx,
        obj,
        &io.helpers.object_layout,
        &io.helpers.array_layout,
        io.helpers.heap_field_offset,
        slow,
    );
    let off = offset as i32;
    let m = MemFlags::trusted();
    let pair = match TypeLayout::of_field(tag).repr {
        ScalarRepr::Bool => {
            let b8 = b.ins().load(types::I8, m, data_base, off);
            let v = b.ins().uextend(types::I64, b8);
            emit::box_bool(b, v)
        }
        ScalarRepr::I64 => {
            let v = b.ins().load(types::I64, m, data_base, off);
            emit::box_int(b, v)
        }
        ScalarRepr::F64 => {
            let f = b.ins().load(types::F64, m, data_base, off);
            emit::box_f64(b, f)
        }
        ScalarRepr::Ref => {
            let raw = b.ins().load(types::I64, m, data_base, off);
            let is_null = b.ins().icmp_imm(IntCC::Equal, raw, COMPACT_REF_NULL as i64);
            let null_tag = b.ins().iconst(types::I64, KIND_NULL as i64);
            let heap_tag = b.ins().iconst(types::I64, KIND_HEAP as i64);
            let zero = b.ins().iconst(types::I64, 0);
            let tag_v = b.ins().select(is_null, null_tag, heap_tag);
            let payload = b.ins().select(is_null, zero, raw);
            b.ins().iconcat(tag_v, payload)
        }
        ScalarRepr::Boxed => b.ins().load(types::I128, m, data_base, off),
    };
    b.ins().jump(cont, &[pair.into()]);

    b.switch_to_block(slow);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        io.cc,
        io.helpers.get_fixed_field,
        &[io.exec_ctx, obj_tag, obj_payload, slot_v],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        io.exec_ctx,
        io.helpers.jit_native_result_offset as i32,
    );
    b.ins().jump(cont, &[res.into()]);

    b.switch_to_block(cont);
    b.block_params(cont)[0]
}

/// Write boxed `value` into the compact field at `offset` of boxed receiver
/// `obj`. Only a nursery receiver is written inline: an old-generation one
/// needs the write barrier the `set_fixed_field` helper carries, and so does
/// anything that is not a compact instance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn store_compact(
    b: &mut FunctionBuilder,
    io: &FieldIo,
    obj: Value,
    value: Value,
    offset: u32,
    tag: Option<varn_core::RuntimeKind>,
    slot: usize,
) {
    let slow = b.create_block();
    let cont = b.create_block();
    let inline = b.create_block();

    // Split both operands here, above the branch: the inline and the slow
    // path both read them.
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let (value_tag, payload) = b.ins().isplit(value);
    let kind = b.ins().band_imm(obj_tag, emit::KIND_MASK);
    let heap_ok = b.ins().icmp_imm(IntCC::Equal, kind, KIND_HEAP as i64);
    let raw = b.ins().band_imm(obj_payload, 0xFFFF_FFFF);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);
    let not_old = b.ins().icmp_imm(IntCC::Equal, old_bit, 0);
    let can_inline = b.ins().band(heap_ok, not_old);
    b.ins().brif(can_inline, inline, &[], slow, &[]);

    b.switch_to_block(inline);
    let data_base = emit::emit_object_data_base(
        b,
        io.exec_ctx,
        obj,
        &io.helpers.object_layout,
        &io.helpers.array_layout,
        io.helpers.heap_field_offset,
        slow,
    );
    let off = offset as i32;
    let m = MemFlags::new();
    match TypeLayout::of_field(tag).repr {
        ScalarRepr::Bool => {
            b.ins().istore8(m, payload, data_base, off);
        }
        ScalarRepr::I64 => {
            b.ins().store(m, payload, data_base, off);
        }
        ScalarRepr::F64 => {
            let f = unbox_f64_coerce(b, value);
            b.ins().store(m, f, data_base, off);
        }
        ScalarRepr::Ref => {
            let is_null = b.ins().icmp_imm(IntCC::Equal, value_tag, KIND_NULL as i64);
            let null_niche = b.ins().iconst(types::I64, COMPACT_REF_NULL as i64);
            let stored = b.ins().select(is_null, null_niche, payload);
            b.ins().store(m, stored, data_base, off);
        }
        ScalarRepr::Boxed => {
            b.ins().store(m, value, data_base, off);
        }
    }
    b.ins().jump(cont, &[]);

    b.switch_to_block(slow);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        io.cc,
        io.helpers.set_fixed_field,
        &[
            io.exec_ctx,
            obj_tag,
            obj_payload,
            slot_v,
            value_tag,
            payload,
        ],
    );
    b.ins().jump(cont, &[]);

    b.switch_to_block(cont);
}

impl FldCtx<'_> {
    fn io(&self) -> FieldIo<'_> {
        FieldIo {
            helpers: self.helpers,
            cc: self.cc,
            exec_ctx: self.exec_ctx,
        }
    }
}

/// `GetFixedField first_reg, obj, slot` on a compact class field.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_get(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    first_reg: usize,
    obj_r: usize,
    offset: u32,
    tag: Option<varn_core::RuntimeKind>,
    slot: usize,
) -> Result<(), String> {
    let obj = match actx {
        Some(actx) => super::super::alloc::box_or_load_home(b, actx, state, obj_r),
        None => use_boxed(b, c.vars, state, obj_r)?,
    };
    let res = load_compact(b, &c.io(), obj, offset, tag, slot);
    match actx {
        Some(actx) => super::super::alloc::def_result(b, actx, first_reg, res),
        None => emit::def_boxed_leaf(b, c.register_meta, c.vars, first_reg, res),
    }
    Ok(())
}

/// `SetFixedField obj(=first_reg), val` on a compact class field.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_set(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    first_reg: usize,
    val_r: usize,
    offset: u32,
    tag: Option<varn_core::RuntimeKind>,
    slot: usize,
) -> Result<(), String> {
    let obj = match actx {
        Some(actx) => super::super::alloc::box_or_load_home(b, actx, state, first_reg),
        None => use_boxed(b, c.vars, state, first_reg)?,
    };
    let val = match actx {
        Some(actx) => super::super::alloc::box_or_load_home(b, actx, state, val_r),
        None => box_or_pass(b, c.vars, state, val_r),
    };
    let val128 = if b.func.dfg.value_type(val) == types::I128 {
        val
    } else {
        emit::box_int(b, val)
    };
    store_compact(b, &c.io(), obj, val128, offset, tag, slot);
    Ok(())
}
