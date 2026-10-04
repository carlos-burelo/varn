use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::call_helper_void;
use super::super::heap::{boxed_parts, exec_ctx};
use super::super::Ctx;

pub(crate) fn emit_array_length(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(b, ctx.cc, ctx.helpers.array_length, &[ectx, tag, payload]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(crate) fn emit_str_length(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    Ok(super::super::super::strings::str_length_boxed(
        b,
        ctx.cc,
        ctx.helpers.str_length,
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
        tag,
        payload,
    ))
}

pub(crate) fn emit_bytes_length(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(b, ctx.cc, ctx.helpers.bytes_length, &[ectx, tag, payload]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
