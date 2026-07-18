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
        | OpCode::ModFloat
        | OpCode::PowFloat
        | OpCode::DivInt
        | OpCode::PowInt
        | OpCode::BitAnd
        | OpCode::BitOr
        | OpCode::BitXor
        | OpCode::Shl
        | OpCode::Shr
        | OpCode::Ushr => emit_binary_arith(ctx, op, first_reg),
        OpCode::ModInt => emit_mod_int(ctx, first_reg),
        OpCode::AddImm | OpCode::SubImm => emit_add_sub_imm(ctx, op, first_reg),
        OpCode::AddInt | OpCode::SubInt | OpCode::MulInt => {
            let mapped_op = match op {
                OpCode::AddInt => OpCode::Add,
                OpCode::SubInt => OpCode::Sub,
                OpCode::MulInt => OpCode::Mul,
                _ => unreachable!(),
            };
            emit_binary_arith(ctx, mapped_op, first_reg);
        }
        OpCode::ToString | OpCode::IsNull | OpCode::Not | OpCode::Negate => {
            return super::unary::emit_unary(ctx, op, first_reg);
        }
        _ => unreachable!("emit_arith called with {:?}", op),
    }
    Ok(())
}

fn emit_binary_arith(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    let is_fast_math = matches!(op, OpCode::Add | OpCode::Sub | OpCode::Mul);

    if is_fast_math {
        let asm = &mut ctx.asm;
        let regmap = &ctx.regmap;
        let helpers = ctx.helpers;

        emit_load(asm, Reg::Rax, src1, regmap);
        emit_load(asm, Reg::R11, src2, regmap);

        // Check src1 tag
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.push(Reg::Rcx);
        asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
        asm.pop(Reg::Rcx);
        let p_fallback1 = asm.jmp_cond(crate::assembler::Cond::NotEqual);

        // Check src2 tag
        asm.mov_reg_reg(Reg::R10, Reg::R11);
        asm.push(Reg::Rcx);
        asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
        asm.pop(Reg::Rcx);
        let p_fallback2 = asm.jmp_cond(crate::assembler::Cond::NotEqual);

        // Extract 48-bit integers to full 64-bit signed integers
        asm.shl_reg_imm8(Reg::Rax, 16);
        asm.sar_reg_imm8(Reg::Rax, 16);
        asm.shl_reg_imm8(Reg::R11, 16);
        asm.sar_reg_imm8(Reg::R11, 16);

        // Perform arithmetic operation in 64-bit
        match op {
            OpCode::Add => asm.add_reg_reg(Reg::Rax, Reg::R11),
            OpCode::Sub => asm.sub_reg_reg(Reg::Rax, Reg::R11),
            OpCode::Mul => asm.imul_reg_reg(Reg::Rax, Reg::R11),
            _ => unreachable!(),
        }

        // Mask to 48 bits and tag. The mask IS the i48 wrap semantics
        // (varn_core::numeric): no overflow guard, no boxed fallback.
        asm.push(Reg::Rcx);
        asm.mov_reg_imm64(Reg::Rcx, 0x0000_FFFF_FFFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::Rcx);
        asm.pop(Reg::Rcx);
        asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
        emit_store(asm, Reg::Rax, first_reg, regmap);
        let p_skip_ffi = asm.jmp_near();

        // Slow path: call FFI helper
        let fallback_pos = asm.current_offset();
        let disp1 = (fallback_pos as i32 - (p_fallback1 as i32 + 4)) as u32;
        asm.patch_u32(p_fallback1, disp1);
        let disp2 = (fallback_pos as i32 - (p_fallback2 as i32 + 4)) as u32;
        asm.patch_u32(p_fallback2, disp2);

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
        emit_reload_all_except(asm, regmap, Some(first_reg));
        let end_pos = asm.current_offset();
        let disp_skip = (end_pos as i32 - (p_skip_ffi as i32 + 4)) as u32;
        asm.patch_u32(p_skip_ffi, disp_skip);
    } else {
        let asm = &mut ctx.asm;
        let regmap = &ctx.regmap;
        let helpers = ctx.helpers;
        let is_gc_free = matches!(
            op,
            OpCode::BitAnd
                | OpCode::BitOr
                | OpCode::BitXor
                | OpCode::Shl
                | OpCode::Shr
                | OpCode::Ushr
        );

        if !is_gc_free {
            emit_flush_all(asm, regmap);
        }

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
            OpCode::Mod | OpCode::ModFloat | OpCode::ModInt => helpers.modulo,
            OpCode::Pow | OpCode::PowFloat | OpCode::PowInt => helpers.pow,
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

        if !is_gc_free {
            emit_reload_all_except(asm, regmap, Some(first_reg));
        }
    }
}

fn emit_mod_int(ctx: &mut CodegenCtx, first_reg: usize) {
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

    let mut slow_patches: Vec<usize> = Vec::new();

    // ALWAYS emit tag guards because of boxed integers
    asm.mov_reg_reg(Reg::R10, Reg::Rax);
    asm.push(Reg::Rcx);
    asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R10, Reg::Rcx);
    asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
    asm.pop(Reg::Rcx);
    slow_patches.push(asm.jmp_cond(crate::assembler::Cond::NotEqual));

    asm.mov_reg_reg(Reg::R10, Reg::R11);
    asm.push(Reg::Rcx);
    asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R10, Reg::Rcx);
    asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
    asm.pop(Reg::Rcx);
    slow_patches.push(asm.jmp_cond(crate::assembler::Cond::NotEqual));

    // Sign-extend the 48-bit divisor payload and guard against zero.
    asm.shl_reg_imm8(Reg::R11, 16);
    asm.sar_reg_imm8(Reg::R11, 16);
    asm.test_reg_reg(Reg::R11, Reg::R11);
    slow_patches.push(asm.jmp_cond(crate::assembler::Cond::Equal));

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.sar_reg_imm8(Reg::Rax, 16);

    // idiv writes RDX, which carries a live ABI register between ops.
    asm.push(Reg::Rdx);
    asm.cqo();
    asm.idiv_reg(Reg::R11);
    asm.mov_reg_reg(Reg::Rax, Reg::Rdx);
    asm.pop(Reg::Rdx);

    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.shr_reg_imm8(Reg::Rax, 16);
    asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
    emit_store(asm, Reg::Rax, first_reg, regmap);
    let p_done = asm.jmp_near();

    // Non-int operand or zero divisor: the helper handles both (raising
    // "modulo by zero" when applicable).
    let slow = asm.current_offset();
    for p in slow_patches {
        asm.patch_u32(p, (slow as i32 - (p as i32 + 4)) as u32);
    }

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
    asm.mov_reg_imm64(Reg::R10, helpers.modulo as u64);
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

    let end = asm.current_offset();
    asm.patch_u32(p_done, (end as i32 - (p_done as i32 + 4)) as u32);
}

fn emit_add_sub_imm(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src = (w1 >> 8) as usize;
    let imm = (w1 & 0xFF) as i8 as i32;

    emit_load(asm, Reg::Rax, src, regmap);

    // Check tag
    asm.mov_reg_reg(Reg::R10, Reg::Rax);
    asm.push(Reg::Rcx);
    asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R10, Reg::Rcx);
    asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
    asm.pop(Reg::Rcx);
    let p_fallback = asm.jmp_cond(crate::assembler::Cond::NotEqual);

    // Extract
    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.sar_reg_imm8(Reg::Rax, 16);

    let adjust = if op == OpCode::SubImm { -imm } else { imm };
    asm.add_reg_imm32(Reg::Rax, adjust);

    // Mask to 48 bits and tag — the mask IS the i48 wrap semantics
    // (varn_core::numeric): no overflow guard, no boxed fallback.
    asm.push(Reg::Rcx);
    asm.mov_reg_imm64(Reg::Rcx, 0x0000_FFFF_FFFF_FFFFu64);
    asm.and_reg_reg(Reg::Rax, Reg::Rcx);
    asm.pop(Reg::Rcx);
    asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
    emit_store(asm, Reg::Rax, first_reg, regmap);
    let p_done = asm.jmp_near();

    // Slow path
    let slow = asm.current_offset();
    let disp1 = (slow as i32 - (p_fallback as i32 + 4)) as u32;
    asm.patch_u32(p_fallback, disp1);

    emit_flush_all(asm, regmap);
    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);
    let need_dummy = regmap.used_phys.len() % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }
    emit_load(asm, Reg::Rax, src, regmap);
    
    // Construct inline VmValue for immediate
    let imm_vmvalue = (0x7FFC_0000_0000_0000u64) | ((imm as i64) as u64 & 0x0000_FFFF_FFFF_FFFFu64);
    asm.mov_reg_imm64(Reg::R11, imm_vmvalue);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);
    asm.mov_reg_reg(ARG_BASE, Reg::R11);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax);
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    let helper_addr = if op == OpCode::SubImm { helpers.sub } else { helpers.add };
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

    let end = asm.current_offset();
    asm.patch_u32(p_done, (end as i32 - (p_done as i32 + 4)) as u32);
}
