use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{
    emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store,
};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_indexing(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::GetIndex | OpCode::ArrayGetIndex => emit_get_index(ctx, first_reg),
        OpCode::SetIndex | OpCode::ArraySetIndex => emit_set_index(ctx, first_reg),
        OpCode::Typeof => emit_typeof(ctx, first_reg),
        OpCode::Instanceof => emit_instanceof(ctx, first_reg),
        _ => unreachable!("emit_indexing called with {:?}", op),
    }
    Ok(())
}

fn emit_get_index(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let obj_reg = (w1 >> 8) as usize;
    let key_reg = (w1 & 0xFF) as usize;

    // Load values into scratch regs BEFORE pushing live regs so we don't clobber them
    emit_load(asm, Reg::R10, obj_reg, regmap);
    emit_load(asm, Reg::R11, key_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    // Windows x64 ABI: rcx=ctx, rdx=obj, r8=key — RSP must be 16-byte aligned before call
    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_reg(Reg::Rcx, ARG_EXEC_CTX);
    asm.mov_reg_reg(Reg::Rdx, Reg::R10);
    asm.mov_reg_reg(Reg::R8, Reg::R11);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_array_get_fast as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    if need_dummy {
        asm.pop(Reg::R11);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::Rax, first_reg, regmap);
}

fn emit_set_index(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let idx_reg = (w1 >> 8) as usize;
    let val_reg = (w1 & 0xFF) as usize;
    let obj_reg = first_reg;

    emit_flush_all(asm, regmap);
    // Load values into scratch regs BEFORE pushing live regs so we don't clobber them
    emit_load(asm, Reg::R10, obj_reg, regmap);
    emit_load(asm, Reg::R11, idx_reg, regmap);
    emit_load(asm, Reg::R12, val_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    // Windows x64 ABI: rcx=ctx, rdx=obj, r8=key, r9=val — RSP must be 16-byte aligned before call
    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_reg(Reg::Rcx, ARG_EXEC_CTX);
    asm.mov_reg_reg(Reg::Rdx, Reg::R10);
    asm.mov_reg_reg(Reg::R8, Reg::R11);
    asm.mov_reg_reg(Reg::R9, Reg::R12);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.jit_array_set_fast as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    if need_dummy {
        asm.pop(Reg::R11);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_reload_all(asm, regmap);
}

fn emit_typeof(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;

    emit_flush_all(asm, regmap);

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

    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.typeof_val as u64);
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

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, first_reg, regmap);
    emit_reload_all_except(asm, regmap, Some(first_reg));
}

fn emit_instanceof(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    emit_load(asm, Reg::Rax, src1, regmap);
    emit_load(asm, Reg::R11, src2, regmap);

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

    asm.mov_reg_reg(ARG_BASE, Reg::R11);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.instanceof as u64);
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

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, first_reg, regmap);
}
