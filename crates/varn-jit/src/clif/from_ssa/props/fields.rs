use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::call_helper_void;
use super::super::super::fields::{
    cached_base, load_compact, store_compact, store_compact_native, FieldIo,
};
use super::super::heap::{boxed_value, exec_ctx};
use super::super::store::{load_value, Out};
use super::super::Ctx;

fn field_io<'a>(ctx: &'a Ctx<'_>) -> Result<FieldIo<'a>, String> {
    Ok(FieldIo {
        helpers: ctx.helpers,
        cc: ctx.cc,
        exec_ctx: exec_ctx(ctx),
    })
}
pub(crate) fn emit_get_fixed_field(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    slot: u16,
    offset: u32,
    access: varn_core::FieldAccess,
    dest: Option<u32>,
) -> Result<Out, String> {
    use varn_core::layout::{ScalarRepr, TypeLayout};
    use varn_types::register_meta::SlotKind;
    let obj = boxed_value(b, ctx, values, object)?;
    let ivar = ctx.views.instance_var(ctx.views.canon(object));
    if let (Some(d), varn_core::FieldAccess::Compact(tag)) = (dest, access) {
        let native = matches!(
            (ctx.ssa.value_ty(d), TypeLayout::of_field(tag).repr),
            (SlotKind::Int, ScalarRepr::I64)
                | (SlotKind::Float, ScalarRepr::F64)
                | (SlotKind::Bool, ScalarRepr::Bool)
        );
        if native {
            let v = emit_get_fixed_field_native(
                b,
                ctx,
                obj,
                ivar,
                offset,
                tag,
                slot,
                ctx.ssa.value_ty(d),
            )?;
            return Ok(Out::Native(v));
        }
    }
    if let varn_core::FieldAccess::Compact(kind) = access {
        return Ok(Out::Boxed(load_compact(
            b,
            &field_io(ctx)?,
            ivar,
            obj,
            offset,
            kind,
            slot as usize,
        )));
    }
    let (ot, op) = b.ins().isplit(obj);
    let ectx = exec_ctx(ctx);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.get_fixed_field,
        &[ectx, ot, op, slot_v],
    );
    Ok(Out::Boxed(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    )))
}

fn emit_get_fixed_field_native(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    obj: Value,
    ivar: Option<cranelift_frontend::Variable>,
    offset: u32,
    tag: Option<varn_core::RuntimeKind>,
    slot: u16,
    dest: varn_types::register_meta::SlotKind,
) -> Result<Value, String> {
    use super::super::heap::unbox_dest;
    use varn_core::layout::{ScalarRepr, TypeLayout};
    let h = ctx.helpers;
    let ectx = exec_ctx(ctx);
    let merge_ty = match dest {
        varn_types::register_meta::SlotKind::Float => types::F64,
        varn_types::register_meta::SlotKind::Int
        | varn_types::register_meta::SlotKind::Bool
        | varn_types::register_meta::SlotKind::Str
        | varn_types::register_meta::SlotKind::Ref
        | varn_types::register_meta::SlotKind::Dynamic => types::I64,
    };
    let slow = b.create_block();
    let cont = b.create_block();
    b.append_block_param(cont, merge_ty);
    let data_base = cached_base(b, ivar, &field_io(ctx)?, obj, slow);
    let off = offset as i32;
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let v = match TypeLayout::of_field(tag).repr {
        ScalarRepr::I64 => b.ins().load(types::I64, m, data_base, off),
        ScalarRepr::F64 => b.ins().load(types::F64, m, data_base, off),
        ScalarRepr::Bool => {
            let b8 = b.ins().load(types::I8, m, data_base, off);
            b.ins().uextend(types::I64, b8)
        }
        ScalarRepr::Ref | ScalarRepr::Boxed => {
            return Err("from_ssa: native field read of a non-scalar repr".into());
        }
    };
    b.ins().jump(cont, &[v.into()]);
    b.switch_to_block(slow);
    let (ot, op) = b.ins().isplit(obj);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(b, ctx.cc, h.get_fixed_field, &[ectx, ot, op, slot_v]);
    let boxed = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        h.jit_native_result_offset as i32,
    );
    let back = unbox_dest(b, dest, boxed)?;
    b.ins().jump(cont, &[back.into()]);
    b.switch_to_block(cont);
    Ok(b.block_params(cont)[0])
}
pub(crate) fn emit_set_fixed_field(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    value: u32,
    slot: u16,
    offset: u32,
    kind: Option<varn_core::RuntimeKind>,
) -> Result<(), String> {
    use varn_core::layout::{ScalarRepr, TypeLayout};
    use varn_types::register_meta::SlotKind;
    let obj = boxed_value(b, ctx, values, object)?;
    let ivar = ctx.views.instance_var(ctx.views.canon(object));
    let vk = ctx.ssa.value_ty(value);
    let repr = TypeLayout::of_field(kind).repr;
    let native = matches!(
        (vk, repr),
        (SlotKind::Int, ScalarRepr::I64)
            | (SlotKind::Bool, ScalarRepr::Bool)
            | (SlotKind::Bool, ScalarRepr::I64)
            | (SlotKind::Int, ScalarRepr::Bool)
            | (SlotKind::Float, ScalarRepr::F64)
            | (SlotKind::Int, ScalarRepr::F64)
    );
    if native {
        let raw = load_value(b, ctx, values, value)?;
        store_compact_native(
            b,
            &field_io(ctx)?,
            ivar,
            obj,
            raw,
            matches!(vk, SlotKind::Float),
            offset,
            kind,
            slot as usize,
        );
        return Ok(());
    }
    let val = boxed_value(b, ctx, values, value)?;
    store_compact(
        b,
        &field_io(ctx)?,
        ivar,
        obj,
        val,
        offset,
        kind,
        slot as usize,
    );
    Ok(())
}
