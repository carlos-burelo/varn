use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

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

            // GC safepoint on the back-edge, mirroring the interpreter's Loop
            // handler: a long call-free allocating loop must still reach the
            // collector before the nursery overflows into the old generation.
            // Fast path: two loads + compare (nursery fill level).
            let helpers = ctx.helpers;
            asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, helpers.heap_field_offset as i32);
            asm.mov_reg_mem(Reg::R10, Reg::R10, helpers.nursery_len_offset as i32);
            asm.mov_reg_imm64(Reg::R11, helpers.nursery_threshold as u64);
            asm.cmp_reg_reg(Reg::R10, Reg::R11);
            let skip_gc = asm.jmp_cond(Cond::Less);

            emit_flush_all(asm, regmap);
            asm.push(ARG_CTX);
            asm.push(ARG_CLOSURE);
            asm.push(ARG_BASE);
            asm.push(ARG_EXEC_CTX);
            let need_dummy = regmap.used_phys.len() % 2 == 0;
            if need_dummy {
                asm.push(Reg::Rax);
            }
            asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
            #[cfg(target_os = "windows")]
            asm.add_reg_imm8(Reg::Rsp, -32);
            asm.mov_reg_imm64(Reg::R10, helpers.gc_safepoint as u64);
            asm.call_reg(Reg::R10);
            #[cfg(target_os = "windows")]
            asm.add_reg_imm8(Reg::Rsp, 32);
            if need_dummy {
                asm.pop(Reg::Rax);
            }
            asm.pop(ARG_EXEC_CTX);
            asm.pop(ARG_BASE);
            asm.pop(ARG_CLOSURE);
            asm.pop(ARG_CTX);
            emit_reload_all(asm, regmap);

            let after_gc = asm.current_offset();
            asm.patch_u32(skip_gc, (after_gc as i32 - (skip_gc as i32 + 4)) as u32);

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
