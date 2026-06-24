use crate::assembler::Reg;
use crate::codegen::CodegenCtx;
use crate::regalloc::{
    emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store,
};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

pub(crate) fn emit_assert_not_null(ctx: &mut CodegenCtx, _first_reg: usize) {
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

    asm.mov_reg_imm64(Reg::R10, helpers.assert_not_null as u64);
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

pub(crate) fn emit_close_upvalue(ctx: &mut CodegenCtx, _first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let lowest = (w1 >> 8) as usize;

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

    asm.mov_reg_imm64(ARG_CLOSURE, lowest as u64);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.close_upvalue as u64);
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

pub(crate) fn emit_get_enum_tag(ctx: &mut CodegenCtx, first_reg: usize) {
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

    asm.mov_reg_imm64(Reg::R10, helpers.get_enum_tag as u64);
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

pub(crate) fn emit_is_array(ctx: &mut CodegenCtx, first_reg: usize) {
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

    asm.mov_reg_imm64(Reg::R10, helpers.is_array as u64);
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

pub(crate) fn emit_wrap_spread(ctx: &mut CodegenCtx, first_reg: usize) {
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

    asm.mov_reg_imm64(Reg::R10, helpers.wrap_spread as u64);
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

pub(crate) fn emit_object_keys(ctx: &mut CodegenCtx, first_reg: usize) {
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

    asm.mov_reg_imm64(Reg::R10, helpers.object_keys as u64);
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

pub(crate) fn emit_op_in(ctx: &mut CodegenCtx, first_reg: usize) {
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

    asm.mov_reg_imm64(Reg::R10, helpers.op_in as u64);
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

pub(crate) fn emit_object_merge(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, first_reg, regmap);
    emit_load(asm, Reg::R11, src, regmap);

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

    asm.mov_reg_imm64(Reg::R10, helpers.object_merge as u64);
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

pub(crate) fn emit_bind_method(ctx: &mut CodegenCtx, _first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let name_idx = code[*ip] as usize;
    *ip += 1;
    let dest = (w1 >> 8) as usize;
    let obj_reg = (w1 & 0xFF) as usize;

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

    asm.mov_reg_imm64(Reg::R10, helpers.bind_method as u64);
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

pub(crate) fn emit_make_enum_variant(ctx: &mut CodegenCtx, _first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let ip_before = *ip;
    let w1 = code[*ip];
    *ip += 1;
    let dest = (w1 >> 8) as usize;
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

    asm.mov_reg_imm64(ARG_CLOSURE, ip_before as u64);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.make_enum_variant as u64);
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

pub(crate) fn emit_call_spread(ctx: &mut CodegenCtx, _first_reg: usize) {
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

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, callee_reg, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 5) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, dest as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_count as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, arg_start as u64);
    asm.push(Reg::R11);

    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.call_spread as u64);
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

    asm.mov_reg_mem(ARG_CTX, ARG_EXEC_CTX, 8);

    asm.mov_reg_reg(crate::registers::REG_FRAME_BASE, crate::registers::ARG_BASE);
    asm.shl_reg_imm8(crate::registers::REG_FRAME_BASE, 3);
    asm.add_reg_reg(crate::registers::REG_FRAME_BASE, ARG_CTX);

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

pub(crate) fn emit_load_module(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let spec_idx = code[*ip] as usize;
    *ip += 1;

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

    asm.mov_reg_imm64(ARG_BASE, spec_idx as u64);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.load_module_by_idx as u64);
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

pub(crate) fn emit_store_module_slot(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let slot_idx = code[*ip] as usize;
    *ip += 1;

    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, first_reg, regmap);

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

    asm.mov_reg_reg(ARG_BASE, Reg::Rax);
    asm.mov_reg_imm64(ARG_CLOSURE, slot_idx as u64);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.store_module_slot as u64);
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

    emit_reload_all_except(asm, regmap, None);
}

pub(crate) fn emit_spawn(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 & 0xFF) as usize;
    let dest = first_reg;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, src, regmap);

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

    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.spawn as u64);
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

    emit_store(asm, Reg::R11, dest, regmap);
    emit_reload_all_except(asm, regmap, Some(dest));
}

pub(crate) fn emit_try(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let err_reg = (w1 >> 8) as usize;
    let offset_hi = code[*ip] as u32;
    *ip += 1;
    let offset_lo = code[*ip] as u32;
    *ip += 1;

    let catch_offset = ((offset_hi << 16) | offset_lo) as usize;
    let catch_ip = *ip + catch_offset;

    let _ = first_reg;

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

    asm.mov_reg_imm64(ARG_BASE, err_reg as u64);
    asm.mov_reg_imm64(ARG_CLOSURE, catch_ip as u64);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.try_push as u64);
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

    emit_reload_all_except(asm, regmap, None);
}

pub(crate) fn emit_pop_try(ctx: &mut CodegenCtx) {
    let asm = &mut ctx.asm;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

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

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.try_pop as u64);
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

    emit_reload_all_except(asm, regmap, None);
}

pub(crate) fn emit_throw(ctx: &mut CodegenCtx) {
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

    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

    asm.mov_reg_imm64(Reg::R10, helpers.throw as u64);
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

    emit_reload_all_except(asm, regmap, None);
}

pub(crate) fn emit_await(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;
    let dest = first_reg;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, src, regmap);

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

    asm.mov_reg_reg(Reg::R11, ARG_EXEC_CTX);

    asm.mov_reg_imm64(ARG_EXEC_CTX, *ip as u64);
    asm.mov_reg_imm64(ARG_BASE, dest as u64);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, Reg::R11);

    asm.mov_reg_imm64(Reg::R10, helpers.await_helper as u64);
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

    emit_reload_all_except(asm, regmap, None);
}

pub(crate) fn emit_yield(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 & 0xFF) as usize;
    let dest = first_reg;

    emit_flush_all(asm, regmap);
    emit_load(asm, Reg::Rax, src, regmap);

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

    asm.mov_reg_reg(Reg::R11, ARG_EXEC_CTX);

    asm.mov_reg_imm64(ARG_EXEC_CTX, *ip as u64);
    asm.mov_reg_imm64(ARG_BASE, dest as u64);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, Reg::R11);

    asm.mov_reg_imm64(Reg::R10, helpers.yield_helper as u64);
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

    emit_reload_all_except(asm, regmap, None);
}
