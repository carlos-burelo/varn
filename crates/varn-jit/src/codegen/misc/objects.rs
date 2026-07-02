use crate::assembler::Reg;
use crate::codegen::CodegenCtx;
use crate::regalloc::{
    emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store,
};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

pub(crate) fn emit_build_object(ctx: &mut CodegenCtx, _first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let ip_before = *ip;
    let w1 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let count = (w1 & 0xFF) as usize;
    *ip += count * 2;

    emit_flush_all(asm, regmap);

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

    asm.mov_reg_imm64(ARG_CLOSURE, ip_before as u64);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.build_object as u64);
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

pub(crate) fn emit_object_rest(ctx: &mut CodegenCtx, _first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let ip_before = *ip;
    let w1 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let w2 = code[*ip];
    *ip += 1;
    let skip_count = (w2 >> 8) as usize;
    *ip += skip_count;

    emit_flush_all(asm, regmap);

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

    asm.mov_reg_imm64(ARG_CLOSURE, ip_before as u64);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.object_rest as u64);
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

pub(crate) fn emit_get_fixed_field(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let obj_reg = (code[*ip] >> 8) as usize;
    *ip += 1;
    let slot = code[*ip] as usize;
    *ip += 1;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, obj_reg, regmap);

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

    asm.mov_reg_imm64(ARG_BASE, slot as u64);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.get_fixed_field as u64);
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

pub(crate) fn emit_set_fixed_field(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let val_reg = (code[*ip] >> 8) as usize;
    *ip += 1;
    let slot = code[*ip] as usize;
    *ip += 1;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, first_reg, regmap);
    emit_load(asm, Reg::R11, val_reg, regmap);

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
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_imm64(ARG_BASE, slot as u64);
    asm.mov_reg_reg(ARG_EXEC_CTX, Reg::R11);

    asm.mov_reg_imm64(Reg::R10, helpers.set_fixed_field as u64);
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

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_reload_all(asm, regmap);
}

pub(crate) fn emit_get_property_maybe(ctx: &mut CodegenCtx, first_reg: usize) {
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

    asm.mov_reg_imm64(Reg::R10, helpers.get_property_maybe as u64);
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

pub(crate) fn emit_get_super(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let name_idx = code[*ip] as usize;
    *ip += 1;

    emit_flush_all(asm, regmap);

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

    asm.mov_reg_imm64(ARG_CLOSURE, name_idx as u64);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.get_super as u64);
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

pub(crate) fn emit_get_symbol(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let obj_reg = (code[*ip] >> 8) as usize;
    *ip += 1;
    let sym_idx = code[*ip] as usize;
    *ip += 1;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, obj_reg, regmap);

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

    asm.mov_reg_imm64(ARG_BASE, sym_idx as u64);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.get_symbol as u64);
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
