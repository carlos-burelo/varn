use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::call_helper_void;
use super::super::heap::{boxed_parts, exec_ctx};
use super::super::Ctx;

pub(crate) fn emit_array_push(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    array: u32,
    value: u32,
) -> Result<(), String> {
    let (at, ap) = boxed_parts(b, ctx, values, array)?;
    let (vt, vp) = boxed_parts(b, ctx, values, value)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(b, ctx.cc, ctx.helpers.array_push, &[ectx, at, ap, vt, vp]);
    Ok(())
}

pub(crate) fn emit_get_index(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
) -> Result<Value, String> {
    let (ot, op) = boxed_parts(b, ctx, values, object)?;
    let (kt, kp) = boxed_parts(b, ctx, values, index)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_array_get_fast,
        &[ectx, ot, op, kt, kp],
    );
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(crate) fn emit_set_index(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
    value: u32,
) -> Result<(), String> {
    let (ot, op) = boxed_parts(b, ctx, values, object)?;
    let (kt, kp) = boxed_parts(b, ctx, values, index)?;
    let (vt, vp) = boxed_parts(b, ctx, values, value)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_array_set_fast,
        &[ectx, ot, op, kt, kp, vt, vp],
    );
    Ok(())
}
