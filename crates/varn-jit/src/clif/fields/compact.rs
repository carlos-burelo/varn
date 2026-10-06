




use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, Value};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::FunctionBuilder;
use varn_core::layout::{ScalarRepr, TypeLayout, COMPACT_REF_NULL};
use varn_types::vm_value::{KIND_HEAP, KIND_NULL};

use super::super::emit::{self, call_helper_void, unbox_f64_coerce};
use crate::JitHelpers;


pub(crate) struct FieldIo<'a> {
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: Value,
}




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

    let data_base = emit::emit_instance_payload(
        b,
        obj,
        &io.helpers.object_layout,
        &io.helpers.array_layout,
        slow,
    );
    let off = offset as i32;
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
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
            let is_null = b
                .ins()
                .icmp_imm_u(IntCC::Equal, raw, COMPACT_REF_NULL as i64);
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
        cranelift_codegen::ir::MemFlagsData::trusted(),
        io.exec_ctx,
        io.helpers.jit_native_result_offset as i32,
    );
    b.ins().jump(cont, &[res.into()]);

    b.switch_to_block(cont);
    b.block_params(cont)[0]
}





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

    
    
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let (value_tag, payload) = b.ins().isplit(value);
    let kind = b.ins().band_imm_u(obj_tag, emit::KIND_MASK);
    let heap_ok = b.ins().icmp_imm_u(IntCC::Equal, kind, KIND_HEAP as i64);
    let heap = b.create_block();
    b.ins().brif(heap_ok, heap, &[], slow, &[]);
    b.switch_to_block(heap);
    let young = emit::is_young(b, obj_payload, &io.helpers.array_layout);
    b.ins().brif(young, inline, &[], slow, &[]);

    b.switch_to_block(inline);
    let data_base = emit::emit_instance_payload(
        b,
        obj,
        &io.helpers.object_layout,
        &io.helpers.array_layout,
        slow,
    );
    let off = offset as i32;
    let m = cranelift_codegen::ir::MemFlagsData::new();
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
            let is_null = b
                .ins()
                .icmp_imm_u(IntCC::Equal, value_tag, KIND_NULL as i64);
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
