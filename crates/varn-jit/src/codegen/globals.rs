use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{
    emit_load, emit_store,
};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_globals(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    match op {
        OpCode::LoadGlobalIdx => {
            let idx = code[*ip] as usize;
            *ip += 1;

            use crate::assembler::Cond;

            // R10 = ExecCtx.globals.values.len (offset 64)
            asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 64);
            asm.cmp_reg_imm32(Reg::R10, idx as i32);
            let fallback_patch = asm.jmp_cond(Cond::BelowEqual);

            // Fast path: load values ptr (offset 56) and read from it
            asm.mov_reg_mem(Reg::R11, ARG_EXEC_CTX, 56);
            asm.mov_reg_mem(Reg::R11, Reg::R11, (idx * 8) as i32);
            emit_store(asm, Reg::R11, first_reg, regmap);
            let end_patch = asm.jmp_near();

            // Fallback path:
            let fallback_pos = asm.current_offset();
            let relative_fallback = (fallback_pos as i64 - (fallback_patch as i64 + 4)) as i32;
            asm.patch_u32(fallback_patch, relative_fallback as u32);
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

            asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
            asm.mov_reg_imm64(ARG_CLOSURE, idx as u64);

            asm.mov_reg_imm64(Reg::R10, helpers.load_global_idx as u64);
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

            // End patch
            let end_pos = asm.current_offset();
            let relative_end = (end_pos as i64 - (end_patch as i64 + 4)) as i32;
            asm.patch_u32(end_patch, relative_end as u32);
        }
        OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
            let w1 = code[*ip];
            *ip += 1;
            let src = (w1 >> 8) as usize;
            let idx = code[*ip] as usize;
            *ip += 1;

            use crate::assembler::Cond;

            // Load value to store into Rax
            emit_load(asm, Reg::Rax, src, regmap);

            // R10 = ExecCtx.globals.values.len (offset 64)
            asm.mov_reg_mem(Reg::R10, ARG_EXEC_CTX, 64);
            asm.cmp_reg_imm32(Reg::R10, idx as i32);
            let fallback_patch = asm.jmp_cond(Cond::BelowEqual);

            // Fast path: load values ptr (offset 56) and write to ptr[idx]
            asm.mov_reg_mem(Reg::R11, ARG_EXEC_CTX, 56);
            asm.mov_mem_reg(Reg::R11, (idx * 8) as i32, Reg::Rax);
            let end_patch = asm.jmp_near();

            // Fallback path:
            let fallback_pos = asm.current_offset();
            let relative_fallback = (fallback_pos as i64 - (fallback_patch as i64 + 4)) as i32;
            asm.patch_u32(fallback_patch, relative_fallback as u32);
            emit_load(asm, Reg::Rax, src, regmap);

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

            asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
            asm.mov_reg_imm64(ARG_CLOSURE, idx as u64);
            asm.mov_reg_reg(ARG_BASE, Reg::Rax);

            let helper_addr = if op == OpCode::StoreGlobalIdx {
                helpers.store_global_idx
            } else {
                helpers.define_global_idx
            };

            asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
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

            // End patch
            let end_pos = asm.current_offset();
            let relative_end = (end_pos as i64 - (end_patch as i64 + 4)) as i32;
            asm.patch_u32(end_patch, relative_end as u32);
        }
        OpCode::LoadGlobal => {
            let idx = code[*ip] as usize;
            *ip += 1;
            // Reload closure pointer from saved stack slot
            asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

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

            asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
            asm.mov_reg_imm64(ARG_BASE, idx as u64);

            asm.mov_reg_imm64(Reg::R10, helpers.load_global as u64);
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
        _ => unreachable!("emit_globals called with {:?}", op),
    }
    Ok(())
}

