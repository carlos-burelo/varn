use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::{emit_load, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_compare(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::Eq
        | OpCode::Neq
        | OpCode::Lt
        | OpCode::Lte
        | OpCode::Gt
        | OpCode::Gte
        | OpCode::EqFloat
        | OpCode::NeqFloat
        | OpCode::LtFloat
        | OpCode::LteFloat
        | OpCode::GtFloat
        | OpCode::GteFloat => emit_float_compare(ctx, op, first_reg),
        OpCode::LtInt
        | OpCode::GtInt
        | OpCode::LteInt
        | OpCode::GteInt
        | OpCode::EqInt
        | OpCode::NeqInt => emit_int_compare(ctx, op, first_reg),
        _ => unreachable!("emit_compare called with {:?}", op),
    }
    Ok(())
}

fn emit_float_compare(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = regmap.used_phys.len() % 2 != 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    emit_load(asm, Reg::Rax, src1, regmap);
    emit_load(asm, Reg::R11, src2, regmap);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_reg(ARG_BASE, Reg::R11);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    let helper_addr = match op {
        OpCode::Eq | OpCode::EqFloat => helpers.eq,
        OpCode::Neq | OpCode::NeqFloat => helpers.neq,
        OpCode::Lt | OpCode::LtFloat => helpers.lt,
        OpCode::Lte | OpCode::LteFloat => helpers.lte,
        OpCode::Gt | OpCode::GtFloat => helpers.gt,
        OpCode::Gte | OpCode::GteFloat => helpers.gte,
        _ => unreachable!(),
    };

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
}

fn emit_int_compare(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;

    let w1 = code[*ip];
    *ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    emit_load(asm, Reg::Rax, src1, regmap);
    emit_load(asm, Reg::R11, src2, regmap);

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.sar_reg_imm8(Reg::Rax, 16);
    asm.shl_reg_imm8(Reg::R11, 16);
    asm.sar_reg_imm8(Reg::R11, 16);

    asm.cmp_reg_reg(Reg::Rax, Reg::R11);

    let cond = match op {
        OpCode::LtInt => Cond::Less,
        OpCode::GtInt => Cond::Greater,
        OpCode::LteInt => Cond::LessEqual,
        OpCode::GteInt => Cond::GreaterEqual,
        OpCode::EqInt => Cond::Equal,
        OpCode::NeqInt => Cond::NotEqual,
        _ => unreachable!(),
    };

    let jmp_true_patch = asm.jmp_cond(cond);

    let false_val = 0x7FFA_0000_0000_0000u64;
    asm.mov_reg_imm64(Reg::Rax, false_val);
    let jmp_end_patch = asm.jmp_near();

    let true_pos = asm.current_offset();
    let disp_true = (true_pos as i32 - (jmp_true_patch as i32 + 4)) as u32;
    asm.patch_u32(jmp_true_patch, disp_true);

    let true_val = 0x7FFB_0000_0000_0000u64;
    asm.mov_reg_imm64(Reg::Rax, true_val);

    let end_pos = asm.current_offset();
    let disp_end = (end_pos as i32 - (jmp_end_patch as i32 + 4)) as u32;
    asm.patch_u32(jmp_end_patch, disp_end);

    emit_store(asm, Reg::Rax, first_reg, regmap);
}
