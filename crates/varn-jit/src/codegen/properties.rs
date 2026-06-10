use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{
    emit_flush_all, emit_load, emit_reload_all, emit_reload_all_except, emit_store,
};
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

    // Fast-path JIT inline cache lookup (no spill)
    emit_load(asm, Reg::Rax, obj_reg, regmap);
    // Reload closure pointer into R11 scratch register from saved stack slot [Rsp + 8]
    asm.mov_reg_mem(Reg::R11, Reg::Rsp, 8);

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

    // standard x64 args: ctx, closure, obj, cs_idx
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::R11);
    asm.mov_reg_reg(ARG_BASE, Reg::Rax);
    asm.mov_reg_imm64(ARG_EXEC_CTX, cs_idx as u64);

    asm.mov_reg_imm64(Reg::R10, helpers.get_property_ic_fast as u64);
    asm.call_reg(Reg::R10);

    // Save return value to R11 to prevent clobbering by stack popping
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    // Restore return value to Rax
    asm.mov_reg_reg(Reg::Rax, Reg::R11);

    // Check if the result is the VM_UNDEFINED sentinel (0x7FF8_0000_0000_0000)
    asm.mov_reg_imm64(Reg::R11, 0x7FF8_0000_0000_0000);
    asm.cmp_reg_reg(Reg::Rax, Reg::R11);

    use crate::assembler::Cond;
    let fallback_patch = asm.jmp_cond(Cond::Equal);

    // Fast-path hit: store RAX to register and jump to end
    emit_store(asm, Reg::Rax, first_reg, regmap);
    let end_patch = asm.jmp_near();

    // Fallback path:
    let fallback_pos = asm.current_offset();
    let relative_fallback = (fallback_pos as i64 - (fallback_patch as i64 + 4)) as i32;
    asm.patch_u32(fallback_patch, relative_fallback as u32);

    // Slow path: full spill/reload original get_property
    emit_flush_all(asm, regmap);

    emit_load(asm, Reg::Rax, obj_reg, regmap);

    // Reload closure pointer from saved stack slot
    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let slow_need_dummy = (regmap.used_phys.len() + 5) % 2 == 0;
    if slow_need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R11, *ip as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, first_reg as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, cs_idx as u64);
    asm.push(Reg::R11);

    asm.mov_reg_imm64(Reg::R11, name_idx as u64);
    asm.push(Reg::R11);

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

    if slow_need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    emit_store(asm, Reg::R11, first_reg, regmap);
    emit_reload_all_except(asm, regmap, Some(first_reg));

    // Patch the fast path jump to end of instruction
    let end_pos = asm.current_offset();
    let relative_end = (end_pos as i64 - (end_patch as i64 + 4)) as i32;
    asm.patch_u32(end_patch, relative_end as u32);
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

    // Reload closure pointer from saved stack slot
    asm.mov_reg_mem(ARG_CLOSURE, Reg::Rsp, 8);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + 5) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    asm.mov_reg_imm64(Reg::R10, *ip as u64);
    asm.push(Reg::R10);

    asm.mov_reg_imm64(Reg::R10, cs_idx as u64);
    asm.push(Reg::R10);

    asm.mov_reg_imm64(Reg::R10, name_idx as u64);
    asm.push(Reg::R10);

    asm.push(Reg::R11);

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
