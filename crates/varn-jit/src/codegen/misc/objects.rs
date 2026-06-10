use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};
use crate::codegen::CodegenCtx;

/// BuildObject: ip+1+count*2 (variable)
/// w1: dest=hi(w1), count=lo(w1); followed by count*(key_idx, val_reg) pairs
/// Passes ip_offset (position of w1) to helper
/// Signature: jit_build_object(ctx, ip_offset) -> VmValue
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
    *ip += count * 2; // skip all key+val pairs

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

/// ObjectRest: ip+2+skip_count (variable)
/// w1: dest=hi(w1), src=lo(w1); w2: skip_count=hi(w2); followed by skip_count key indices
/// Signature: jit_object_rest(ctx, ip_offset) -> VmValue
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
    *ip += skip_count; // skip the key indices

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

/// GetFixedField: ip+2 (obj_reg=hi(code[ip]), slot=code[ip+1]), dest=first_reg
/// Signature: jit_get_fixed_field(ctx, obj, slot) -> VmValue
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

    emit_store(asm, Reg::R11, first_reg, regmap);
    emit_reload_all_except(asm, regmap, Some(first_reg));
}

/// SetFixedField: ip+2 (val_reg=hi(code[ip]), slot=code[ip+1]), obj=first_reg
/// Signature: jit_set_fixed_field(ctx, obj, slot, val) -> void
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
    emit_load(asm, Reg::Rax, first_reg, regmap); // obj
    emit_load(asm, Reg::R11, val_reg, regmap);   // val

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

    // set_fixed_field(ctx, obj, slot, val) - 4 args
    // Must set ARG_CTX = ARG_EXEC_CTX FIRST before overwriting ARG_EXEC_CTX
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);     // arg1 = ctx
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);      // arg2 = obj
    asm.mov_reg_imm64(ARG_BASE, slot as u64);    // arg3 = slot
    asm.mov_reg_reg(ARG_EXEC_CTX, Reg::R11);     // arg4 = val

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

    emit_reload_all(asm, regmap);
}

/// GetPropertyMaybe: ip+2 (obj_reg=hi(code[ip]), name_idx=code[ip+1]), dest=first_reg
/// Signature: jit_get_property_maybe(ctx, obj, name_idx) -> VmValue
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

/// GetSuper: ip+1 (name_idx=code[ip]), dest=first_reg
/// Signature: jit_get_super(ctx, name_idx) -> VmValue
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

/// GetSymbol: ip+2 (obj_reg=hi(code[ip]), sym_idx=code[ip+1]), dest=first_reg
/// Signature: jit_get_symbol(ctx, obj, sym_idx) -> VmValue
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
