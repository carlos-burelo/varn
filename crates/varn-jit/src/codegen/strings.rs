use varn_core::OpCode;

use super::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use super::CodegenCtx;

pub(crate) fn emit_strings(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::StrConcat => emit_binary_str(ctx, ctx.helpers.str_concat, first_reg),
        OpCode::StrSlice => emit_binary_str(ctx, ctx.helpers.str_slice, first_reg),
        OpCode::StrLength => emit_str_length(ctx, first_reg),
        _ => unreachable!("emit_strings called with {:?}", op),
    }
    Ok(())
}

fn emit_str_length(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.str_length,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

/// StrConcat / StrSlice: `(ctx, a, b) -> value`.
fn emit_binary_str(ctx: &mut CodegenCtx, helper: usize, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper,
            args: &[FfiArg::Vreg(src1), FfiArg::Vreg(src2)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}
