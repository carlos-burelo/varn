use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_epilogue, emit_load, emit_store};
use crate::safepoint::{emit_load_reg, emit_store_reg};

use super::CodegenCtx;

pub(crate) fn emit_immediates(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;

    match op {
        OpCode::LoadNull => {
            let null_val = 0x7FF9_0000_0000_0000u64;
            asm.mov_reg_imm64(Reg::Rax, null_val);
            emit_store(asm, Reg::Rax, first_reg, regmap);
        }
        OpCode::LoadTrue => {
            let true_val = 0x7FFB_0000_0000_0000u64;
            asm.mov_reg_imm64(Reg::Rax, true_val);
            emit_store(asm, Reg::Rax, first_reg, regmap);
        }
        OpCode::LoadFalse => {
            let false_val = 0x7FFA_0000_0000_0000u64;
            asm.mov_reg_imm64(Reg::Rax, false_val);
            emit_store(asm, Reg::Rax, first_reg, regmap);
        }
        OpCode::LoadInt => {
            let val = code[*ip] as i16;
            *ip += 1;
            let int_val = 0x7FFC_0000_0000_0000u64 | (val as u64 & 0x0000_FFFF_FFFF_FFFFu64);
            asm.mov_reg_imm64(Reg::Rax, int_val);
            emit_store(asm, Reg::Rax, first_reg, regmap);
        }
        OpCode::LoadIntZero => {
            let int_val = 0x7FFC_0000_0000_0000u64;
            asm.mov_reg_imm64(Reg::Rax, int_val);
            emit_store(asm, Reg::Rax, first_reg, regmap);
        }
        OpCode::LoadIntOne => {
            let int_val = 0x7FFC_0000_0000_0001u64;
            asm.mov_reg_imm64(Reg::Rax, int_val);
            emit_store(asm, Reg::Rax, first_reg, regmap);
        }
        OpCode::LoadIntMinusOne => {
            let int_val = 0x7FFC_FFFF_FFFF_FFFFu64;
            asm.mov_reg_imm64(Reg::Rax, int_val);
            emit_store(asm, Reg::Rax, first_reg, regmap);
        }
        OpCode::Move => {
            let w1 = code[*ip];
            *ip += 1;
            let src = (w1 >> 8) as usize;
            let src_phys = regmap.get(src);
            let dest_phys = regmap.get(first_reg);
            match (src_phys, dest_phys) {
                (Some(s), Some(d)) => {
                    if s != d {
                        asm.mov_reg_reg(d, s);
                    }
                }
                (Some(s), None) => {
                    emit_store_reg(asm, s, first_reg);
                }
                (None, Some(d)) => {
                    emit_load_reg(asm, d, src);
                }
                (None, None) => {
                    if src != first_reg {
                        emit_load_reg(asm, Reg::Rax, src);
                        emit_store_reg(asm, Reg::Rax, first_reg);
                    }
                }
            }
        }
        OpCode::Return => {
            let w1 = code[*ip];
            *ip += 1;
            let src = (w1 & 0xFF) as usize;
            emit_load(asm, Reg::Rax, src, regmap);

            asm.mov_reg_reg(Reg::R11, Reg::Rax);
            emit_epilogue(asm, regmap);
            asm.mov_reg_reg(Reg::Rax, Reg::R11);
            asm.ret();
        }
        OpCode::LoadConst => {
            emit_load_const(ctx, first_reg);
        }
        _ => unreachable!("emit_immediates called with {:?}", op),
    }
    Ok(())
}

fn emit_load_const(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;

    let idx = code[*ip] as usize;
    *ip += 1;

    let const_val = ctx.constants[idx].0;

    asm.mov_reg_imm64(Reg::Rax, const_val);
    emit_store(asm, Reg::Rax, first_reg, regmap);
}
