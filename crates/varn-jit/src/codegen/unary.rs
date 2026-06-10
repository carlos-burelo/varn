use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_unary(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) -> Result<(), String> {
    match op {
        OpCode::ToString => emit_to_string(ctx, first_reg),
        OpCode::IsNull => emit_is_null(ctx, first_reg),
        OpCode::Not => emit_not(ctx, first_reg),
        OpCode::Negate => emit_negate(ctx, first_reg),
        _ => unreachable!("emit_unary called with {:?}", op),
    }
    Ok(())
}

fn emit_to_string(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;

    emit_flush_all(asm, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    emit_load(asm, Reg::Rax, src, regmap);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    let helper_addr = helpers.to_string;
    asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    emit_store(asm, Reg::R11, first_reg, regmap);

    emit_reload_all_except(asm, regmap, Some(first_reg));
}

fn emit_is_null(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;

    let src = (code[*ip] >> 8) as usize;
    *ip += 1;

    emit_load(asm, Reg::Rax, src, regmap);

    asm.mov_reg_imm64(Reg::R11, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::Rax, Reg::R11);

    asm.mov_reg_imm64(Reg::R11, 0x7FF9_0000_0000_0000u64);
    asm.cmp_reg_reg(Reg::Rax, Reg::R11);

    let patch_je = asm.jmp_cond(Cond::Equal);

    asm.mov_reg_imm64(Reg::Rax, 0x7FFA_0000_0000_0000u64);
    let patch_jmp = asm.jmp_near();

    let true_pos = asm.current_offset();
    let disp_je = (true_pos as i32 - (patch_je as i32 + 4)) as u32;
    asm.patch_u32(patch_je, disp_je);

    asm.mov_reg_imm64(Reg::Rax, 0x7FFB_0000_0000_0000u64);

    let end_pos = asm.current_offset();
    let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
    asm.patch_u32(patch_jmp, disp_jmp);

    emit_store(asm, Reg::Rax, first_reg, regmap);
}

fn emit_not(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;

    emit_load(asm, Reg::Rax, src, regmap);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);
    asm.mov_reg_imm64(Reg::R10, 0x7FFE_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R11, Reg::R10);
    asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64);
    asm.cmp_reg_reg(Reg::R11, Reg::R10);

    let patch_je = asm.jmp_cond(Cond::Equal);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    emit_flush_all(asm, regmap);
    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_reg(ARG_CLOSURE, Reg::R11);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    let helper_addr = helpers.logical_not;
    asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    emit_reload_all_except(asm, regmap, Some(first_reg));
    let patch_jmp = asm.jmp_near();

    let bool_pos = asm.current_offset();
    let disp_je = (bool_pos as i32 - (patch_je as i32 + 4)) as u32;
    asm.patch_u32(patch_je, disp_je);

    asm.mov_reg_imm64(Reg::R10, 0x0001_0000_0000_0000u64);
    asm.xor_reg_reg(Reg::Rax, Reg::R10);
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    let end_pos = asm.current_offset();
    let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
    asm.patch_u32(patch_jmp, disp_jmp);

    emit_store(asm, Reg::R11, first_reg, regmap);
}

fn emit_negate(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;

    emit_load(asm, Reg::Rax, src, regmap);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);
    asm.mov_reg_imm64(Reg::R10, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R11, Reg::R10);
    asm.cmp_reg_reg(Reg::R11, crate::registers::REG_INT_TAG);

    let patch_je = asm.jmp_cond(Cond::Equal);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    emit_flush_all(asm, regmap);
    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_reg(ARG_CLOSURE, Reg::R11);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    let helper_addr = helpers.negate;
    asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    emit_reload_all_except(asm, regmap, Some(first_reg));
    let patch_jmp = asm.jmp_near();

    let int_pos = asm.current_offset();
    let disp_je = (int_pos as i32 - (patch_je as i32 + 4)) as u32;
    asm.patch_u32(patch_je, disp_je);

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.sar_reg_imm8(Reg::Rax, 16);

    asm.mov_reg_reg(Reg::R10, Reg::Rax);
    asm.mov_reg_imm64(Reg::Rax, 0);
    asm.sub_reg_reg(Reg::Rax, Reg::R10);

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.shr_reg_imm8(Reg::Rax, 16);
    asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    let end_pos = asm.current_offset();
    let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
    asm.patch_u32(patch_jmp, disp_jmp);

    emit_store(asm, Reg::R11, first_reg, regmap);
}
