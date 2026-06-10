use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_arith(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) -> Result<(), String> {
    match op {
        OpCode::Add
        | OpCode::Sub
        | OpCode::Mul
        | OpCode::Div
        | OpCode::Mod
        | OpCode::Pow
        | OpCode::AddFloat
        | OpCode::SubFloat
        | OpCode::MulFloat
        | OpCode::DivFloat
        | OpCode::DivInt
        | OpCode::BitAnd
        | OpCode::BitOr
        | OpCode::BitXor
        | OpCode::Shl
        | OpCode::Shr
        | OpCode::Ushr => emit_binary_arith(ctx, op, first_reg),
        OpCode::AddImm | OpCode::SubImm => emit_add_sub_imm(ctx, op, first_reg),
        OpCode::AddInt | OpCode::SubInt | OpCode::MulInt => emit_int_arith(ctx, op, first_reg),
        OpCode::ToString | OpCode::IsNull | OpCode::Not | OpCode::Negate => {
            return super::unary::emit_unary(ctx, op, first_reg);
        }
        _ => unreachable!("emit_arith called with {:?}", op),
    }
    Ok(())
}

fn emit_binary_arith(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    let is_fast_math = matches!(op, OpCode::Add | OpCode::Sub | OpCode::Mul);

    if is_fast_math {
        // --- 1. Load operands to RAX and R11 ---
        emit_load(asm, Reg::Rax, src1, regmap);
        emit_load(asm, Reg::R11, src2, regmap);

        // --- 2. Check if src1 (RAX) is an integer ---
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
        let p_fallback1 = asm.jmp_cond(crate::assembler::Cond::NotEqual);

        // --- 3. Check if src2 (R11) is an integer ---
        asm.mov_reg_reg(Reg::R10, Reg::R11);
        asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
        let p_fallback2 = asm.jmp_cond(crate::assembler::Cond::NotEqual);

        // --- 4. FAST PATH (both are integers) ---
        // Sign-extend the 48-bit payloads in RAX and R11
        asm.shl_reg_imm8(Reg::Rax, 16);
        asm.sar_reg_imm8(Reg::Rax, 16);
        asm.shl_reg_imm8(Reg::R11, 16);
        asm.sar_reg_imm8(Reg::R11, 16);

        // Perform integer operation
        match op {
            OpCode::Add => asm.add_reg_reg(Reg::Rax, Reg::R11),
            OpCode::Sub => asm.sub_reg_reg(Reg::Rax, Reg::R11),
            OpCode::Mul => asm.imul_reg_reg(Reg::Rax, Reg::R11),
            _ => unreachable!(),
        }

        // Pack RAX back into VmValue NaN-boxed integer format
        asm.shl_reg_imm8(Reg::Rax, 16);
        asm.shr_reg_imm8(Reg::Rax, 16);
        asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);

        // Store RAX to destination register
        emit_store(asm, Reg::Rax, first_reg, regmap);

        // Skip the fallback FFI block
        let p_skip_ffi = asm.jmp_near();

        // --- 5. SLOW PATH (FFI fallback) ---
        let fallback_pos = asm.current_offset();

        // Patch conditional jumps to target fallback_pos
        let disp1 = (fallback_pos as i32 - (p_fallback1 as i32 + 4)) as u32;
        asm.patch_u32(p_fallback1, disp1);

        let disp2 = (fallback_pos as i32 - (p_fallback2 as i32 + 4)) as u32;
        asm.patch_u32(p_fallback2, disp2);

        // Standard FFI call block
        emit_flush_all(asm, regmap);

        asm.push(ARG_CTX);
        asm.push(ARG_CLOSURE);
        asm.push(ARG_BASE);
        asm.push(ARG_EXEC_CTX);

        let need_dummy = regmap.used_phys.len() % 2 == 0;
        if need_dummy {
            asm.push(Reg::Rax);
        }

        emit_load(asm, Reg::Rax, src1, regmap);
        emit_load(asm, Reg::R11, src2, regmap);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, -32);

        asm.mov_reg_reg(ARG_BASE, Reg::R11);
        asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
        asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

        let helper_addr = match op {
            OpCode::Add => helpers.add,
            OpCode::Sub => helpers.sub,
            OpCode::Mul => helpers.mul,
            _ => unreachable!(),
        };

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

        emit_store(asm, Reg::R11, first_reg, regmap);

        // Reload phys regs after slow-path FFI (flush_all wrote them before call).
        // Fast path jumps past this reload since phys regs were never flushed.
        emit_reload_all_except(asm, regmap, Some(first_reg));

        // Patch skip_ffi to here — fast path lands after the reload.
        let end_pos = asm.current_offset();
        let disp_skip = (end_pos as i32 - (p_skip_ffi as i32 + 4)) as u32;
        asm.patch_u32(p_skip_ffi, disp_skip);
    } else {
        // --- ORIGINAL FFI execution for non-optimized ops ---
        emit_flush_all(asm, regmap);

        asm.push(ARG_CTX);
        asm.push(ARG_CLOSURE);
        asm.push(ARG_BASE);
        asm.push(ARG_EXEC_CTX);

        let need_dummy = regmap.used_phys.len() % 2 == 0;
        if need_dummy {
            asm.push(Reg::Rax);
        }

        emit_load(asm, Reg::Rax, src1, regmap);
        emit_load(asm, Reg::R11, src2, regmap);

        #[cfg(target_os = "windows")]
        asm.add_reg_imm8(Reg::Rsp, -32);

        asm.mov_reg_reg(ARG_BASE, Reg::R11);
        asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
        asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);

        let helper_addr = match op {
            OpCode::Add | OpCode::AddFloat => helpers.add,
            OpCode::Sub | OpCode::SubFloat => helpers.sub,
            OpCode::Mul | OpCode::MulFloat => helpers.mul,
            OpCode::Div | OpCode::DivFloat | OpCode::DivInt => helpers.div,
            OpCode::Mod => helpers.modulo,
            OpCode::Pow => helpers.pow,
            OpCode::BitAnd => helpers.bit_and,
            OpCode::BitOr => helpers.bit_or,
            OpCode::BitXor => helpers.bit_xor,
            OpCode::Shl => helpers.shl,
            OpCode::Shr => helpers.shr,
            OpCode::Ushr => helpers.ushr,
            _ => unreachable!("unoptimized op: {:?}", op),
        };

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

        emit_store(asm, Reg::R11, first_reg, regmap);

        emit_reload_all_except(asm, regmap, Some(first_reg));
    }
}

fn emit_add_sub_imm(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;
    let imm = (w1 & 0xFF) as i8 as i32;

    emit_load(asm, Reg::Rax, src, regmap);

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.sar_reg_imm8(Reg::Rax, 16);

    let adjust = if op == OpCode::SubImm { -imm } else { imm };
    asm.add_reg_imm32(Reg::Rax, adjust);

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.shr_reg_imm8(Reg::Rax, 16);
    asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);

    emit_store(asm, Reg::Rax, first_reg, regmap);
}

fn emit_int_arith(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;

    let w1 = code[*ip];
    *ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    emit_load(asm, Reg::Rax, src1, regmap);
    emit_load(asm, Reg::R11, src2, regmap);

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.sar_reg_imm8(Reg::Rax, 16);
    asm.shl_reg_imm8(Reg::R11, 16);
    asm.sar_reg_imm8(Reg::R11, 16);

    match op {
        OpCode::AddInt => asm.add_reg_reg(Reg::Rax, Reg::R11),
        OpCode::SubInt => asm.sub_reg_reg(Reg::Rax, Reg::R11),
        OpCode::MulInt => asm.imul_reg_reg(Reg::Rax, Reg::R11),
        _ => unreachable!(),
    }

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.shr_reg_imm8(Reg::Rax, 16);
    asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);

    emit_store(asm, Reg::Rax, first_reg, regmap);
}
