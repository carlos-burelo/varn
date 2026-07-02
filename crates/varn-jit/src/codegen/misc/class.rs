use super::NULL_BITS;
use crate::assembler::Reg;
use crate::codegen::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use crate::codegen::CodegenCtx;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

pub(crate) fn emit_declare_field(ctx: &mut CodegenCtx, _first_reg: usize) {
    let obj_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let name_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.declare_field,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Imm(name_idx as u64)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_make_class(ctx: &mut CodegenCtx, first_reg: usize) {
    let super_reg = (ctx.code[ctx.ip] >> 8) as usize;
    ctx.ip += 1;
    let name_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    // Register 0 encodes "no superclass": the helper receives null.
    let super_arg = if super_reg != 0 {
        FfiArg::Vreg(super_reg)
    } else {
        FfiArg::Imm(NULL_BITS)
    };

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.make_class,
            args: &[super_arg, FfiArg::Imm(name_idx as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_inherit(ctx: &mut CodegenCtx, _first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let class_reg = (w1 >> 8) as usize;
    let super_reg = (w1 & 0xFF) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.inherit,
            args: &[FfiArg::Vreg(class_reg), FfiArg::Vreg(super_reg)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

/// Passes a pointer to a `[class, fn, key_idx, kind]` block built on the
/// machine stack, so it stays outside `emit_ffi_call`.
pub(crate) fn emit_class_member_op(ctx: &mut CodegenCtx, _first_reg: usize, kind: u8) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let key_idx = code[*ip] as usize;
    *ip += 1;
    let class_reg = (w1 >> 8) as usize;
    let fn_reg = (w1 & 0xFF) as usize;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, class_reg, regmap);
    emit_load(asm, Reg::R11, fn_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 4) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R10, kind as u64);
    asm.push(Reg::R10);

    asm.mov_reg_imm64(Reg::R10, key_idx as u64);
    asm.push(Reg::R10);

    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.class_member_op as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.add_reg_imm8(Reg::Rsp, 32);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    emit_reload_all(asm, regmap);
}
