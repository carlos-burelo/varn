use super::NULL_BITS;
use crate::assembler::Reg;
use crate::codegen::CodegenCtx;
use crate::regalloc::{
    emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store,
};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

pub(crate) fn emit_declare_field(ctx: &mut CodegenCtx, _first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let obj_reg = (code[*ip] >> 8) as usize;
    *ip += 1;
    let name_idx = code[*ip] as usize;
    *ip += 1;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, obj_reg, regmap);

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

    asm.mov_reg_imm64(ARG_BASE, name_idx as u64);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.declare_field as u64);
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

pub(crate) fn emit_make_class(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let super_reg = (code[*ip] >> 8) as usize;
    *ip += 1;
    let name_idx = code[*ip] as usize;
    *ip += 1;

    emit_flush_all(asm, regmap);

    if super_reg != 0 {
        emit_load(asm, Reg::Rax, super_reg, regmap);
    } else {
        asm.mov_reg_imm64(Reg::Rax, NULL_BITS);
    }

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

    asm.mov_reg_imm64(ARG_BASE, name_idx as u64);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.make_class as u64);
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

pub(crate) fn emit_inherit(ctx: &mut CodegenCtx, _first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let class_reg = (w1 >> 8) as usize;
    let super_reg = (w1 & 0xFF) as usize;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, class_reg, regmap);
    emit_load(asm, Reg::R11, super_reg, regmap);

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

    asm.mov_reg_reg(ARG_BASE, Reg::R11);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.inherit as u64);
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

pub(crate) fn emit_class_member_op(ctx: &mut CodegenCtx, _first_reg: usize, kind: u8) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let key_idx = code[*ip] as usize;
    *ip += 1;
    let class_reg = (w1 >> 8) as usize;
    let fn_reg = (w1 & 0xFF) as usize;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, class_reg, regmap);
    emit_load(asm, Reg::R11, fn_reg, regmap);

    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 4) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R10, kind as u64);
    asm.push(Reg::R10);

    asm.mov_reg_imm64(Reg::R10, key_idx as u64);
    asm.push(Reg::R10);

    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.class_member_op as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

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
