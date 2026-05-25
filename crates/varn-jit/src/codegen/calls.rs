use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_calls(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::Call => emit_call(ctx, first_reg),
        OpCode::CallMethod => emit_call_method(ctx, first_reg),
        _ => unreachable!("emit_calls called with {:?}", op),
    }
    Ok(())
}

fn emit_call(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let w2 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let arg_count = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let _ = first_reg; // first_reg not used for Call

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, callee_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 5) % 2 != 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    // Push JitCallArgs struct fields in reverse order:
    // 1. ip
    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);
    // 2. dest
    asm.mov_reg_imm64(Reg::R11, dest as u64);
    asm.push(Reg::R11);
    // 3. arg_count
    asm.mov_reg_imm64(Reg::R11, arg_count as u64);
    asm.push(Reg::R11);
    // 4. arg_start
    asm.mov_reg_imm64(Reg::R11, arg_start as u64);
    asm.push(Reg::R11);
    // 5. callee
    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.call as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    asm.add_reg_imm8(Reg::Rsp, 40);

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

fn emit_call_method(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let cs = first_reg;
    let w1 = code[*ip];
    *ip += 1;
    let name_idx = code[*ip] as usize;
    *ip += 1;
    let w3 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let obj_reg = (w1 & 0xFF) as usize;
    let arg_count = (w3 >> 8) as usize;
    let arg_start = (w3 & 0xFF) as usize;

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, obj_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 7) % 2 != 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    // Push JitCallMethodArgs struct fields in reverse order:
    // 1. ip
    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);
    // 2. dest
    asm.mov_reg_imm64(Reg::R11, dest as u64);
    asm.push(Reg::R11);
    // 3. arg_count
    asm.mov_reg_imm64(Reg::R11, arg_count as u64);
    asm.push(Reg::R11);
    // 4. arg_start
    asm.mov_reg_imm64(Reg::R11, arg_start as u64);
    asm.push(Reg::R11);
    // 5. cs
    asm.mov_reg_imm64(Reg::R11, cs as u64);
    asm.push(Reg::R11);
    // 6. name_idx
    asm.mov_reg_imm64(Reg::R11, name_idx as u64);
    asm.push(Reg::R11);
    // 7. this_val
    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_BASE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.call_method as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    asm.add_reg_imm8(Reg::Rsp, 56);

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
