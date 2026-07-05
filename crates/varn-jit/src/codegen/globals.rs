use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_load, emit_store};

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

            let dest = regmap.get(first_reg).unwrap_or(Reg::R11);
            asm.mov_reg_mem(dest, crate::registers::REG_GLOBALS, (idx * 8) as i32);
            if dest == Reg::R11 {
                emit_store(asm, Reg::R11, first_reg, regmap);
            }
        }
        OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx => {
            let w1 = code[*ip];
            *ip += 1;
            let src = (w1 >> 8) as usize;
            let idx = code[*ip] as usize;
            *ip += 1;

            let val_reg = regmap.get(src).unwrap_or_else(|| {
                emit_load(asm, Reg::Rax, src, regmap);
                Reg::Rax
            });

            asm.mov_mem_reg(crate::registers::REG_GLOBALS, (idx * 8) as i32, val_reg);
        }
        OpCode::LoadGlobal => {
            let idx = code[*ip] as usize;
            *ip += 1;

            super::ffi::emit_ffi_call(
                asm,
                regmap,
                &super::ffi::FfiCallSpec {
                    helper: helpers.load_global,
                    args: &[
                        super::ffi::FfiArg::SavedClosure,
                        super::ffi::FfiArg::Imm(idx as u64),
                    ],
                    flush: false,
                    dest: Some(first_reg),
                    reload: false,
                    recompute_frame: false,
                },
            );
        }
        _ => unreachable!("emit_globals called with {:?}", op),
    }
    Ok(())
}
