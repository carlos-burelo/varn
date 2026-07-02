use varn_core::OpCode;

use super::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use super::CodegenCtx;

pub(crate) fn emit_indexing(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::GetIndex | OpCode::ArrayGetIndex => emit_get_index(ctx, first_reg),
        OpCode::SetIndex | OpCode::ArraySetIndex => emit_set_index(ctx, first_reg),
        OpCode::Typeof => emit_typeof(ctx, first_reg),
        OpCode::Instanceof => emit_instanceof(ctx, first_reg),
        _ => unreachable!("emit_indexing called with {:?}", op),
    }
    Ok(())
}

fn emit_get_index(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let obj_reg = (w1 >> 8) as usize;
    let key_reg = (w1 & 0xFF) as usize;

    // The fast helper neither reads frame slots nor triggers GC on the
    // fast path, so allocated registers stay live across the call; the
    // slow path may grow the stack, hence the frame recompute.
    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.jit_array_get_fast,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Vreg(key_reg)],
            flush: false,
            dest: Some(first_reg),
            reload: false,
            recompute_frame: true,
        },
    );
}

fn emit_set_index(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let idx_reg = (w1 >> 8) as usize;
    let val_reg = (w1 & 0xFF) as usize;
    let obj_reg = first_reg;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.jit_array_set_fast,
            args: &[
                FfiArg::Vreg(obj_reg),
                FfiArg::Vreg(idx_reg),
                FfiArg::Vreg(val_reg),
            ],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: true,
        },
    );
}

fn emit_typeof(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.typeof_val,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: true,
        },
    );
}

fn emit_instanceof(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.instanceof,
            args: &[FfiArg::Vreg(src1), FfiArg::Vreg(src2)],
            flush: false,
            dest: Some(first_reg),
            reload: false,
            recompute_frame: true,
        },
    );
}
