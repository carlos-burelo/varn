use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::emit_load;

use super::{CodegenCtx, JumpPatch};

pub(crate) fn emit_jumps(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) -> Result<(), String> {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let jump_patches = &mut ctx.jump_patches;

    match op {
        OpCode::Jump => {
            let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
            let target_bytecode_ip = *ip + 2 + offset;
            *ip += 2;

            let patch_pos = asm.jmp_near();
            jump_patches.push(JumpPatch {
                patch_pos,
                target_bytecode_ip,
            });
        }
        OpCode::Loop => {
            let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
            let target_bytecode_ip = *ip + 2 - offset;
            *ip += 2;

            let patch_pos = asm.jmp_near();
            jump_patches.push(JumpPatch {
                patch_pos,
                target_bytecode_ip,
            });
        }
        OpCode::JumpIfFalse => {
            let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
            let target_bytecode_ip = *ip + 2 + offset;
            *ip += 2;

            emit_load(asm, Reg::Rax, first_reg, regmap);

            let is_bool = ctx.proto.register_meta.get(first_reg).map_or(false, |m| {
                m.kind == varn_types::register_meta::SlotKind::Bool
            });
            if is_bool {
                asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p1 = asm.jmp_cond(Cond::Equal);
                jump_patches.push(JumpPatch {
                    patch_pos: p1,
                    target_bytecode_ip,
                });
            } else {
                asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p1 = asm.jmp_cond(Cond::Equal);

                asm.mov_reg_imm64(Reg::R10, 0x7FF9_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p2 = asm.jmp_cond(Cond::Equal);

                asm.cmp_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
                let p3 = asm.jmp_cond(Cond::Equal);

                jump_patches.push(JumpPatch {
                    patch_pos: p1,
                    target_bytecode_ip,
                });
                jump_patches.push(JumpPatch {
                    patch_pos: p2,
                    target_bytecode_ip,
                });
                jump_patches.push(JumpPatch {
                    patch_pos: p3,
                    target_bytecode_ip,
                });
            }
        }
        OpCode::JumpIfTrue => {
            let offset = ((code[*ip] as u32) << 16 | code[*ip + 1] as u32) as usize;
            let target_bytecode_ip = *ip + 2 + offset;
            *ip += 2;

            emit_load(asm, Reg::Rax, first_reg, regmap);

            let is_bool = ctx.proto.register_meta.get(first_reg).map_or(false, |m| {
                m.kind == varn_types::register_meta::SlotKind::Bool
            });
            if is_bool {
                asm.mov_reg_imm64(Reg::R10, 0x7FFB_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p1 = asm.jmp_cond(Cond::Equal);
                jump_patches.push(JumpPatch {
                    patch_pos: p1,
                    target_bytecode_ip,
                });
            } else {
                asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p_false = asm.jmp_cond(Cond::Equal);

                asm.mov_reg_imm64(Reg::R10, 0x7FF9_0000_0000_0000u64);
                asm.cmp_reg_reg(Reg::Rax, Reg::R10);
                let p_null = asm.jmp_cond(Cond::Equal);

                asm.cmp_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
                let p_zero = asm.jmp_cond(Cond::Equal);

                let p_target = asm.jmp_near();
                jump_patches.push(JumpPatch {
                    patch_pos: p_target,
                    target_bytecode_ip,
                });

                let falsy_pos = asm.current_offset();
                let disp_false = (falsy_pos as i32 - (p_false as i32 + 4)) as u32;
                asm.patch_u32(p_false, disp_false);

                let disp_null = (falsy_pos as i32 - (p_null as i32 + 4)) as u32;
                asm.patch_u32(p_null, disp_null);

                let disp_zero = (falsy_pos as i32 - (p_zero as i32 + 4)) as u32;
                asm.patch_u32(p_zero, disp_zero);
            }
        }
        _ => unreachable!("emit_jumps called with {:?}", op),
    }
    Ok(())
}
