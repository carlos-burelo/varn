use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_arith(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
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

    // 4. Load the two operands into Rax and R11
    emit_load(asm, Reg::Rax, src1, regmap);
    emit_load(asm, Reg::R11, src2, regmap);

    // 5. Windows ABI Shadow Space
    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    // 6. Set up the three arguments for the extern "C" function:
    asm.mov_reg_reg(ARG_BASE, Reg::R11); // Arg 3 = b
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rax); // Arg 2 = a
    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX); // Arg 1 = exec_ctx

    // 7. Get the corresponding helper address
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
        _ => unreachable!(),
    };

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

    asm.mov_reg_imm64(Reg::R10, 0x0000_FFFF_FFFF_FFFFu64);
    asm.and_reg_reg(Reg::Rax, Reg::R10);
    asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
    asm.or_reg_reg(Reg::Rax, Reg::R10);

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

    asm.mov_reg_imm64(Reg::R10, 0x0000_FFFF_FFFF_FFFFu64);
    asm.and_reg_reg(Reg::Rax, Reg::R10);
    asm.mov_reg_imm64(Reg::R10, 0x7FFC_0000_0000_0000u64);
    asm.or_reg_reg(Reg::Rax, Reg::R10);

    emit_store(asm, Reg::Rax, first_reg, regmap);
}
