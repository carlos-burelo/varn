use crate::codegen::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use crate::codegen::CodegenCtx;

pub(crate) fn emit_build_object(ctx: &mut CodegenCtx, _first_reg: usize) {
    // The helper re-decodes the pair list from the bytecode ip.
    let ip_before = ctx.ip;
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let dest = (w1 >> 8) as usize;
    let count = (w1 & 0xFF) as usize;
    ctx.ip += count * 2;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.build_object,
            args: &[FfiArg::Imm(ip_before as u64)],
            flush: true,
            dest: Some(dest),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_object_rest(ctx: &mut CodegenCtx, _first_reg: usize) {
    let ip_before = ctx.ip;
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let dest = (w1 >> 8) as usize;
    let w2 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let skip_count = (w2 >> 8) as usize;
    ctx.ip += skip_count;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.object_rest,
            args: &[FfiArg::Imm(ip_before as u64)],
            flush: true,
            dest: Some(dest),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_get_fixed_field(ctx: &mut CodegenCtx, first_reg: usize) {
    let obj_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let slot = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.get_fixed_field,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Imm(slot as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: true,
        },
    );
}

pub(crate) fn emit_set_fixed_field(ctx: &mut CodegenCtx, first_reg: usize) {
    let val_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let slot = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.set_fixed_field,
            args: &[
                FfiArg::Vreg(first_reg),
                FfiArg::Imm(slot as u64),
                FfiArg::Vreg(val_reg),
            ],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: true,
        },
    );
}

pub(crate) fn emit_get_property_maybe(ctx: &mut CodegenCtx, first_reg: usize) {
    let obj_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let name_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.get_property_maybe,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Imm(name_idx as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_get_super(ctx: &mut CodegenCtx, first_reg: usize) {
    let name_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.get_super,
            args: &[FfiArg::Imm(name_idx as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_get_symbol(ctx: &mut CodegenCtx, first_reg: usize) {
    let obj_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let sym_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.get_symbol,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Imm(sym_idx as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}
