use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_properties(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::GetProperty => emit_get_property(ctx, first_reg),
        OpCode::SetProperty => emit_set_property(ctx, first_reg),
        _ => unreachable!("emit_properties called with {:?}", op),
    }
    Ok(())
}

fn emit_get_property(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let obj_reg = (w1 >> 8) as usize;
    let cs_idx = (w1 & 0xFF) as usize;
    let name_idx = code[*ip] as usize;
    *ip += 1;

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, obj_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 5) % 2 != 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    // Push JitGetPropertyArgs struct fields in reverse order:
    // 1. ip
    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);
    // 2. dest
    asm.mov_reg_imm64(Reg::R11, first_reg as u64);
    asm.push(Reg::R11);
    // 3. cs_idx
    asm.mov_reg_imm64(Reg::R11, cs_idx as u64);
    asm.push(Reg::R11);
    // 4. name_idx
    asm.mov_reg_imm64(Reg::R11, name_idx as u64);
    asm.push(Reg::R11);
    // 5. obj
    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_BASE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.get_property as u64);
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

    emit_store(asm, Reg::R11, first_reg, regmap);
    emit_reload_all_except(asm, regmap, Some(first_reg));
}

fn emit_set_property(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let val_reg = (w1 >> 8) as usize;
    let cs_idx = (w1 & 0xFF) as usize;
    let name_idx = code[*ip] as usize;
    *ip += 1;
    let obj_reg = first_reg;

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, obj_reg, regmap);
    emit_load(asm, Reg::R11, val_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 5) % 2 != 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    // Push JitSetPropertyArgs struct fields in reverse order:
    // 1. ip
    asm.mov_reg_imm64(Reg::R10, *ip as u64);
    asm.push(Reg::R10);
    // 2. cs_idx
    asm.mov_reg_imm64(Reg::R10, cs_idx as u64);
    asm.push(Reg::R10);
    // 3. name_idx
    asm.mov_reg_imm64(Reg::R10, name_idx as u64);
    asm.push(Reg::R10);
    // 4. val
    asm.push(Reg::R11);
    // 5. obj
    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_BASE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.set_property as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.add_reg_imm8(Reg::Rsp, 40);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    emit_reload_all(asm, regmap);
}
