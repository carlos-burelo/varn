//! FFI-backed misc opcodes. Every regular helper call goes through
//! `codegen::ffi::emit_ffi_call`; only `emit_call_spread` stays
//! hand-written (it passes a pointer to an argument block it builds on the
//! machine stack).

use crate::assembler::Reg;
use crate::codegen::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use crate::codegen::CodegenCtx;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

pub(crate) fn emit_assert_not_null(ctx: &mut CodegenCtx, _first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.assert_not_null,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_close_upvalue(ctx: &mut CodegenCtx, _first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let lowest = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.close_upvalue,
            args: &[FfiArg::Imm(lowest as u64)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

/// `(ctx, value) -> value` helpers that read one register and store one
/// result: GetEnumTag, IsArray, WrapSpread, ObjectKeys.
fn emit_unary_helper(ctx: &mut CodegenCtx, helper: usize, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_get_enum_tag(ctx: &mut CodegenCtx, first_reg: usize) {
    let helper = ctx.helpers.get_enum_tag;
    emit_unary_helper(ctx, helper, first_reg);
}

pub(crate) fn emit_is_array(ctx: &mut CodegenCtx, first_reg: usize) {
    let helper = ctx.helpers.is_array;
    emit_unary_helper(ctx, helper, first_reg);
}

pub(crate) fn emit_wrap_spread(ctx: &mut CodegenCtx, first_reg: usize) {
    let helper = ctx.helpers.wrap_spread;
    emit_unary_helper(ctx, helper, first_reg);
}

pub(crate) fn emit_object_keys(ctx: &mut CodegenCtx, first_reg: usize) {
    let helper = ctx.helpers.object_keys;
    emit_unary_helper(ctx, helper, first_reg);
}

pub(crate) fn emit_op_in(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.op_in,
            args: &[FfiArg::Vreg(src1), FfiArg::Vreg(src2)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_object_merge(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.object_merge,
            args: &[FfiArg::Vreg(first_reg), FfiArg::Vreg(src)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_bind_method(ctx: &mut CodegenCtx, _first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let name_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;
    let dest = (w1 >> 8) as usize;
    let obj_reg = (w1 & 0xFF) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.bind_method,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Imm(name_idx as u64)],
            flush: true,
            dest: Some(dest),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_make_enum_variant(ctx: &mut CodegenCtx, _first_reg: usize) {
    // The helper re-decodes the instruction from the bytecode ip.
    let ip_before = ctx.ip;
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let dest = (w1 >> 8) as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.make_enum_variant,
            args: &[FfiArg::Imm(ip_before as u64)],
            flush: true,
            dest: Some(dest),
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_call_spread(ctx: &mut CodegenCtx, _first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let w2 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let arg_count = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, callee_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 5) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, dest as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_count as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_start as u64);
    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.call_spread as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    asm.add_reg_imm8(Reg::Rsp, 40);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

pub(crate) fn emit_load_module(ctx: &mut CodegenCtx, first_reg: usize) {
    let spec_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.load_module_by_idx,
            args: &[FfiArg::SavedClosure, FfiArg::Imm(spec_idx as u64)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: true,
        },
    );
}

pub(crate) fn emit_store_module_slot(ctx: &mut CodegenCtx, first_reg: usize) {
    let slot_idx = ctx.code[ctx.ip] as usize;
    ctx.ip += 1;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.store_module_slot,
            args: &[FfiArg::Imm(slot_idx as u64), FfiArg::Vreg(first_reg)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_spawn(ctx: &mut CodegenCtx, first_reg: usize) {
    // 2-word shape, task register in the operand word's high byte
    // (varn_types::bytecode::decode is authoritative).
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.spawn,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: true,
        },
    );
}

pub(crate) fn emit_try(ctx: &mut CodegenCtx, _first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let err_reg = (w1 >> 8) as usize;
    let offset_hi = ctx.code[ctx.ip] as u32;
    ctx.ip += 1;
    let offset_lo = ctx.code[ctx.ip] as u32;
    ctx.ip += 1;

    let catch_offset = ((offset_hi << 16) | offset_lo) as usize;
    let catch_ip = ctx.ip + catch_offset;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.try_push,
            args: &[FfiArg::Imm(catch_ip as u64), FfiArg::Imm(err_reg as u64)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_pop_try(ctx: &mut CodegenCtx) {
    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.try_pop,
            args: &[],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_throw(ctx: &mut CodegenCtx) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.throw,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

/// Await/Yield suspend helpers: `(ctx, value, dest_reg, resume_ip)`.
fn emit_suspend(ctx: &mut CodegenCtx, helper: usize, src: usize, dest: usize) {
    let resume_ip = ctx.ip;
    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper,
            args: &[
                FfiArg::Vreg(src),
                FfiArg::Imm(dest as u64),
                FfiArg::Imm(resume_ip as u64),
            ],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

pub(crate) fn emit_await(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;
    let helper = ctx.helpers.await_helper;
    emit_suspend(ctx, helper, src, first_reg);
}

pub(crate) fn emit_yield(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 & 0xFF) as usize;
    let helper = ctx.helpers.yield_helper;
    emit_suspend(ctx, helper, src, first_reg);
}
