use crate::codegen::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use crate::codegen::CodegenCtx;

/// DefineGlobal / StoreGlobal: `(ctx, value, name_idx)`.
fn emit_global_by_name(ctx: &mut CodegenCtx, helper: usize) {
    let src = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let name_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper,
            args: &[FfiArg::Vreg(src), FfiArg::Imm(name_idx as u64)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_define_global(ctx: &mut CodegenCtx, _first_reg: usize) {
    let helper = ctx.helpers.define_global;
    emit_global_by_name(ctx, helper);
}

pub(crate) fn emit_store_global(ctx: &mut CodegenCtx, _first_reg: usize) {
    let helper = ctx.helpers.store_global;
    emit_global_by_name(ctx, helper);
}
