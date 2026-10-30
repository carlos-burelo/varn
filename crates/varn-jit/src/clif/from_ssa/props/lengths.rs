use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::unbox_int;
use super::super::arrays;
use super::super::heap::{boxed_parts, boxed_value, exec_ctx};
use super::super::{Ctx, Out};

pub(crate) fn emit_array_length(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Out, String> {
    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    b.append_block_param(merge, types::I64);

    let (_data, len, _disc) = arrays::view(b, ctx, values, operand, slow)?;
    b.ins().jump(merge, &[len.into()]);

    b.switch_to_block(slow);
    let obj = boxed_value(b, ctx, values, operand)?;
    let (tag, payload) = b.ins().isplit(obj);
    let ectx = exec_ctx(ctx);
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.array_length,
        &[ectx, tag, payload],
    )?;
    let boxed = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    let back = unbox_int(b, boxed);
    b.ins().jump(merge, &[back.into()]);

    b.switch_to_block(merge);
    Ok(Out::Native(b.block_params(merge)[0]))
}

pub(crate) fn emit_str_length(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    let (landing, err_ret) = ctx.exits(b);
    Ok(super::super::super::strings::str_length_boxed(
        b,
        ctx.cc,
        ctx.helpers.str_length,
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
        ctx.helpers.jit_error_flag_offset as i32,
        landing,
        err_ret,
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
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.bytes_length,
        &[ectx, tag, payload],
    )?;
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
