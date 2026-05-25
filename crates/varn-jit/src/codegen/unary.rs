use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_unary(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::ToString => emit_to_string(ctx, first_reg),
        OpCode::IsNull => emit_is_null(ctx, first_reg),
        OpCode::Not => emit_not(ctx, first_reg),
        OpCode::Negate => emit_negate(ctx, first_reg),
        _ => unreachable!("emit_unary called with {:?}", op),
    }
    Ok(())
}

fn emit_to_string(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;

    // 1. Flush physical registers to memory before helper call
    emit_flush_all(asm, regmap);

    // 2. Preserve arg registers
    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    // 3. Align stack to 16 bytes:
    let need_dummy = regmap.used_phys.len() % 2 != 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    // 4. Load the operand into Rax
    emit_load(asm, Reg::Rax, src, regmap);

    // 5. Windows ABI Shadow Space
    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    // 6. Set up the two arguments for the extern "C" function:
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax); // Arg 2 = v
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX); // Arg 1 = exec_ctx

    // 7. Call the helper address
    let helper_addr = helpers.to_string;
    asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
    asm.call_reg(Reg::R10);

    // 8. Restore Windows ABI Shadow Space
    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    // 9. Save result from Rax into R11 before pops clobber Rax
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    // 10. Pop arguments and dummy alignment
    if need_dummy {
        asm.pop(Reg::Rax);
    }
    asm.pop(ARG_EXEC_CTX);
    asm.pop(ARG_BASE);
    asm.pop(ARG_CLOSURE);
    asm.pop(ARG_CTX);

    // 11. Save the result back to virtual register first_reg
    emit_store(asm, Reg::R11, first_reg, regmap);

    // 12. Reload all other physical registers safely from memory
    emit_reload_all_except(asm, regmap, Some(first_reg));
}

fn emit_is_null(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;

    let src = (code[*ip] >> 8) as usize;
    *ip += 1;

    emit_load(asm, Reg::Rax, src, regmap);

    // Mask: (v & 0x7FFF_0000_0000_0000)
    asm.mov_reg_imm64(Reg::R11, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::Rax, Reg::R11);

    // Compare with 0x7FF9_0000_0000_0000u64
    asm.mov_reg_imm64(Reg::R11, 0x7FF9_0000_0000_0000u64);
    asm.cmp_reg_reg(Reg::Rax, Reg::R11);

    // Jump if equal to true_path
    let patch_je = asm.jmp_cond(Cond::Equal);

    // False path: load false_val (0x7FFA_0000_0000_0000u64)
    asm.mov_reg_imm64(Reg::Rax, 0x7FFA_0000_0000_0000u64);
    let patch_jmp = asm.jmp_near();

    // True path: load true_val (0x7FFB_0000_0000_0000u64)
    let true_pos = asm.current_offset();
    let disp_je = (true_pos as i32 - (patch_je as i32 + 4)) as u32;
    asm.patch_u32(patch_je, disp_je);

    asm.mov_reg_imm64(Reg::Rax, 0x7FFB_0000_0000_0000u64);

    // End path
    let end_pos = asm.current_offset();
    let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
    asm.patch_u32(patch_jmp, disp_jmp);

    // Store in first_reg
    emit_store(asm, Reg::Rax, first_reg, regmap);
}

fn emit_not(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;

    // Load operand into Rax
    emit_load(asm, Reg::Rax, src, regmap);

    // Check if the value is a boolean: (v & 0x7FFE_0000_0000_0000) == 0x7FFA_0000_0000_0000
    asm.mov_reg_reg(Reg::R11, Reg::Rax);
    asm.mov_reg_imm64(Reg::R10, 0x7FFE_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R11, Reg::R10);
    asm.mov_reg_imm64(Reg::R10, 0x7FFA_0000_0000_0000u64);
    asm.cmp_reg_reg(Reg::R11, Reg::R10);

    // Jump if equal to the boolean fast path
    let patch_je = asm.jmp_cond(Cond::Equal);

    // --- Slow Path: FFI helper call ---
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    emit_flush_all(asm, regmap);
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

    asm.mov_reg_reg(ARG_CLOSURE, Reg::R11); // Arg 2 = v
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX); // Arg 1 = exec_ctx

    let helper_addr = helpers.logical_not;
    asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
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

    emit_reload_all_except(asm, regmap, Some(first_reg));
    let patch_jmp = asm.jmp_near();

    // --- Fast Path: Boolean toggle ---
    let bool_pos = asm.current_offset();
    let disp_je = (bool_pos as i32 - (patch_je as i32 + 4)) as u32;
    asm.patch_u32(patch_je, disp_je);

    // Toggle bit 48: v ^ 0x0001_0000_0000_0000
    asm.mov_reg_imm64(Reg::R10, 0x0001_0000_0000_0000u64);
    asm.xor_reg_reg(Reg::Rax, Reg::R10);
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    // --- End Path ---
    let end_pos = asm.current_offset();
    let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
    asm.patch_u32(patch_jmp, disp_jmp);

    emit_store(asm, Reg::R11, first_reg, regmap);
}

fn emit_negate(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;

    // Load operand into Rax
    emit_load(asm, Reg::Rax, src, regmap);

    // Check if the value is an integer: (v & 0x7FFF_0000_0000_0000) == 0x7FFC_0000_0000_0000
    asm.mov_reg_reg(Reg::R11, Reg::Rax);
    asm.mov_reg_imm64(Reg::R10, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R11, Reg::R10);
    asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
    asm.cmp_reg_reg(Reg::R11, Reg::R10);

    // Jump if equal to the integer fast path
    let patch_je = asm.jmp_cond(Cond::Equal);

    // --- Slow Path: FFI helper call ---
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    emit_flush_all(asm, regmap);
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

    asm.mov_reg_reg(ARG_CLOSURE, Reg::R11); // Arg 2 = v
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX); // Arg 1 = exec_ctx

    let helper_addr = helpers.negate;
    asm.mov_reg_imm64(Reg::R10, helper_addr as u64);
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

    emit_reload_all_except(asm, regmap, Some(first_reg));
    let patch_jmp = asm.jmp_near();

    // --- Fast Path: Integer Negation ---
    let int_pos = asm.current_offset();
    let disp_je = (int_pos as i32 - (patch_je as i32 + 4)) as u32;
    asm.patch_u32(patch_je, disp_je);

    // Shift left 16, arithmetic shift right 16 to sign extend
    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.sar_reg_imm8(Reg::Rax, 16);

    // Negate: Rax = 0 - Rax
    asm.mov_reg_reg(Reg::R10, Reg::Rax);
    asm.mov_reg_imm64(Reg::Rax, 0);
    asm.sub_reg_reg(Reg::Rax, Reg::R10);

    // Re-tag: (rax & 0x0000_FFFF_FFFF_FFFF) | 0x7FFC_0000_0000_0000
    asm.mov_reg_imm64(Reg::R10, 0x0000_FFFF_FFFF_FFFFu64);
    asm.and_reg_reg(Reg::Rax, Reg::R10);
    asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
    asm.or_reg_reg(Reg::Rax, Reg::R10);
    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    // --- End Path ---
    let end_pos = asm.current_offset();
    let disp_jmp = (end_pos as i32 - (patch_jmp as i32 + 4)) as u32;
    asm.patch_u32(patch_jmp, disp_jmp);

    emit_store(asm, Reg::R11, first_reg, regmap);
}
