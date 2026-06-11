use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_closures(
    ctx: &mut CodegenCtx,
    op: OpCode,
    _first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::LoadUpvalue => emit_load_upvalue(ctx),
        OpCode::StoreUpvalue => emit_store_upvalue(ctx),
        OpCode::MakeClosure => emit_make_closure(ctx),
        OpCode::LoadStaticFn => emit_load_static_fn(ctx),
        _ => unreachable!("emit_closures called with {:?}", op),
    }
    Ok(())
}

fn emit_load_upvalue(ctx: &mut CodegenCtx) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let uv = (w1 & 0xFF) as usize;

    emit_flush_all(asm, regmap);

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
    asm.mov_reg_imm64(ARG_BASE, uv as u64);

    asm.mov_reg_imm64(Reg::R10, helpers.load_upvalue as u64);
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

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

fn emit_store_upvalue(ctx: &mut CodegenCtx) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let uv = (w1 >> 8) as usize;
    let src = (w1 & 0xFF) as usize;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, src, regmap);

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
    asm.mov_reg_imm64(ARG_BASE, uv as u64);
    asm.mov_reg_reg(ARG_EXEC_CTX, Reg::Rax);

    asm.mov_reg_imm64(Reg::R10, helpers.store_upvalue as u64);
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

fn emit_make_closure(ctx: &mut CodegenCtx) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1_ip = *ip;
    let w1 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let uv_count = (w1 & 0xFF) as usize;
    *ip += 1;
    *ip += uv_count;

    emit_flush_all(asm, regmap);

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
    asm.mov_reg_reg(ARG_EXEC_CTX, ARG_BASE);
    asm.mov_reg_imm64(ARG_BASE, w1_ip as u64);

    asm.mov_reg_imm64(Reg::R10, helpers.make_closure as u64);
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

    // Reload ARG_CTX from ExecCtx.stack.ptr (offset 8) in case stack reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

fn emit_load_static_fn(ctx: &mut CodegenCtx) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    // Word 0 is at PC - 1. We already consumed Word 0 (which has dest in bits 15-8).
    // Word 1 is at code[*ip], which is the proto_idx.
    let w1 = code[*ip - 1]; // Word 0
    let dest = (w1 >> 8) as usize;
    let proto_idx = code[*ip] as usize;
    *ip += 1; // consume Word 1

    emit_flush_all(asm, regmap);

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
    // 2nd argument (ARG_CLOSURE) already has the current closure pointer, which is correct.
    asm.mov_reg_imm64(ARG_BASE, proto_idx as u64); // 3rd argument is proto_idx

    asm.mov_reg_imm64(Reg::R10, helpers.load_static_fn as u64);
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

    // Reload ARG_CTX from ExecCtx.stack.ptr (offset 8) in case stack reallocated
    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);
    // Recompute REG_FRAME_BASE = ARG_CTX + ARG_BASE * 8
    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}
