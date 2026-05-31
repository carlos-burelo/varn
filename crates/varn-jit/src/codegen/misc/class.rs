use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};
use crate::codegen::CodegenCtx;
use super::NULL_BITS;

/// DeclareField: ip+2 (obj_reg=hi(w1), name_idx=code[ip+1]), void
/// Signature: jit_declare_field(ctx, class_val, name_idx) -> void
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

/// MakeClass: ip+2 (super_reg=hi(w1), name_idx=code[ip+1]), dest=first_reg
/// Signature: jit_make_class(ctx, super_val, name_idx) -> VmValue
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

    // Load super_val; if super_reg == 0 pass null (no superclass)
    if super_reg != 0 {
        emit_load(asm, Reg::Rax, super_reg, regmap);
    } else {
        asm.mov_reg_imm64(Reg::Rax, NULL_BITS);
    }

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

/// Inherit: ip+1 (w1, class_reg=hi(w1), super_reg=lo(w1)), void
/// Signature: jit_inherit(ctx, class_val, super_val) -> void
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

/// Method/DefineStatic/DefineGetter/DefineSetter/DefineStaticGetter/DefineStaticSetter
/// ip+2 (w1, key_idx=code[ip+1]); class_reg=hi(w1), fn_reg=lo(w1)
/// Signature: jit_class_member_op(ctx, args: *const JitClassMemberArgs) -> void
///
/// JitClassMemberArgs layout (repr(C)):
///   [class_val: u64, fn_val: u64, name_idx: usize, kind: u8]
/// Push order (last pushed = lowest stack address):
///   push kind (as 8-byte), push name_idx (as 8-byte), push fn_val, push class_val
///   => rsp -> class_val, rsp+8 -> fn_val, rsp+16 -> name_idx, rsp+24 -> kind
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

    // Reload closure pointer from saved stack slot
    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    // 3 extra pushes for the struct fields beyond the 2 value pushes below
    // Total extra pushes before call: kind(1) + name_idx(1) + fn_val(1) + class_val(1) = 4
    let need_dummy = (regmap.used_phys.len() + 4) % 2 != 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    // Push struct fields in reverse order so class_val is at lowest address
    // Push kind (u8 value, pushed as u64)
    asm.mov_reg_imm64(Reg::R10, kind as u64);
    asm.push(Reg::R10);
    // Push name_idx
    asm.mov_reg_imm64(Reg::R10, key_idx as u64);
    asm.push(Reg::R10);
    // Push fn_val
    asm.push(Reg::R11);
    // Push class_val (now rsp points to class_val = start of JitClassMemberArgs)
    asm.push(Reg::Rax);

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.class_member_op as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    // Remove the 4 pushed struct fields (4 * 8 = 32 bytes)
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
