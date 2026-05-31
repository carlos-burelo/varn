use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{
    emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store,
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

            emit_flush_all(asm, regmap);

            asm.push(ARG_CTX);
            asm.push(ARG_CLOSURE);
            asm.push(ARG_BASE);
            asm.push(ARG_EXEC_CTX);

            let need_dummy = regmap.used_phys.len() % 2 != 0;
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
            emit_reload_all_except(asm, regmap, Some(first_reg));
        }
        OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
            let w1 = code[*ip];
            *ip += 1;
            let src = (w1 >> 8) as usize;
            let idx = code[*ip] as usize;
            *ip += 1;

            emit_flush_all(asm, regmap);
            emit_load(asm, Reg::Rax, src, regmap);

            asm.push(ARG_CTX);
            asm.push(ARG_CLOSURE);
            asm.push(ARG_BASE);
            asm.push(ARG_EXEC_CTX);

            let need_dummy = regmap.used_phys.len() % 2 != 0;
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

            emit_reload_all(asm, regmap);
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

            let need_dummy = regmap.used_phys.len() % 2 != 0;
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

