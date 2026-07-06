use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::CodegenCtx;

pub(crate) fn emit_compare(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::Eq
        | OpCode::Neq
        | OpCode::Lt
        | OpCode::Lte
        | OpCode::Gt
        | OpCode::Gte
        | OpCode::EqFloat
        | OpCode::NeqFloat
        | OpCode::LtFloat
        | OpCode::LteFloat
        | OpCode::GtFloat
        | OpCode::GteFloat => emit_float_compare(ctx, op, first_reg),
        OpCode::LtInt
        | OpCode::GtInt
        | OpCode::LteInt
        | OpCode::GteInt
        | OpCode::EqInt
        | OpCode::NeqInt => emit_int_compare(ctx, op, first_reg),
        _ => unreachable!("emit_compare called with {:?}", op),
    }
    Ok(())
}

fn slot_is_float(meta: &[varn_types::register_meta::RegisterMeta], reg: usize) -> bool {
    meta.get(reg).map_or(false, |m| {
        m.kind == varn_types::register_meta::SlotKind::Float
    })
}

fn emit_compare_inline(
    asm: &mut crate::assembler::Assembler,
    op: OpCode,
    src1: usize,
    src2: usize,
    first_reg: usize,
    regmap: &crate::regalloc::RegMap,
    both_int: bool,
) {
    emit_load(asm, Reg::Rax, src1, regmap);
    emit_load(asm, Reg::R11, src2, regmap);

    let mut p_true_patches = Vec::new();
    let mut p_false_patches = Vec::new();

    if both_int {
        asm.shl_reg_imm8(Reg::Rax, 16);
        asm.shl_reg_imm8(Reg::R11, 16);
        asm.cmp_reg_reg(Reg::Rax, Reg::R11);

        let cond = match op {
            OpCode::Eq | OpCode::EqFloat | OpCode::EqInt => Cond::Equal,
            OpCode::Neq | OpCode::NeqFloat | OpCode::NeqInt => Cond::NotEqual,
            OpCode::Lt | OpCode::LtFloat | OpCode::LtInt => Cond::Less,
            OpCode::Lte | OpCode::LteFloat | OpCode::LteInt => Cond::LessEqual,
            OpCode::Gt | OpCode::GtFloat | OpCode::GtInt => Cond::Greater,
            OpCode::Gte | OpCode::GteFloat | OpCode::GteInt => Cond::GreaterEqual,
            _ => unreachable!(),
        };
        p_true_patches.push(asm.jmp_cond(cond));
    } else {
        asm.emit_debug_assert_not_int(Reg::Rax);
        asm.emit_debug_assert_not_int(Reg::R11);
        asm.movq_xmm_from_gpr(0, Reg::Rax);
        asm.movq_xmm_from_gpr(1, Reg::R11);
        asm.ucomisd(0, 1);

        match op {
            OpCode::Eq | OpCode::EqFloat => {
                p_false_patches.push(asm.jmp_cond(Cond::Parity));
                p_true_patches.push(asm.jmp_cond(Cond::Equal));
            }
            OpCode::Neq | OpCode::NeqFloat => {
                p_true_patches.push(asm.jmp_cond(Cond::Parity));
                p_true_patches.push(asm.jmp_cond(Cond::NotEqual));
            }
            OpCode::Lt | OpCode::LtFloat => {
                p_false_patches.push(asm.jmp_cond(Cond::Parity));
                p_true_patches.push(asm.jmp_cond(Cond::Below));
            }
            OpCode::Lte | OpCode::LteFloat => {
                p_false_patches.push(asm.jmp_cond(Cond::Parity));
                p_true_patches.push(asm.jmp_cond(Cond::BelowEqual));
            }
            OpCode::Gt | OpCode::GtFloat => {
                p_false_patches.push(asm.jmp_cond(Cond::Parity));
                p_true_patches.push(asm.jmp_cond(Cond::Above));
            }
            OpCode::Gte | OpCode::GteFloat => {
                p_false_patches.push(asm.jmp_cond(Cond::Parity));
                p_true_patches.push(asm.jmp_cond(Cond::AboveEqual));
            }
            _ => unreachable!(),
        }
    }

    let false_pos = asm.current_offset();
    let false_val = 0x7FFA_0000_0000_0000u64;
    asm.mov_reg_imm64(Reg::Rax, false_val);
    let jmp_end_patch = asm.jmp_near();

    let true_pos = asm.current_offset();
    let true_val = 0x7FFB_0000_0000_0000u64;
    asm.mov_reg_imm64(Reg::Rax, true_val);

    let end_pos = asm.current_offset();

    for patch in p_false_patches {
        let disp = (false_pos as i32 - (patch as i32 + 4)) as u32;
        asm.patch_u32(patch, disp);
    }
    for patch in p_true_patches {
        let disp = (true_pos as i32 - (patch as i32 + 4)) as u32;
        asm.patch_u32(patch, disp);
    }
    let disp_end = (end_pos as i32 - (jmp_end_patch as i32 + 4)) as u32;
    asm.patch_u32(jmp_end_patch, disp_end);

    emit_store(asm, Reg::Rax, first_reg, regmap);
}

fn emit_float_compare(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    *ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    let both_float = slot_is_float(&ctx.proto.register_meta, src1)
        && slot_is_float(&ctx.proto.register_meta, src2);

    if both_float {
        emit_compare_inline(asm, op, src1, src2, first_reg, regmap, false);
        return;
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
        OpCode::Eq | OpCode::EqFloat => helpers.eq,
        OpCode::Neq | OpCode::NeqFloat => helpers.neq,
        OpCode::Lt | OpCode::LtFloat => helpers.lt,
        OpCode::Lte | OpCode::LteFloat => helpers.lte,
        OpCode::Gt | OpCode::GtFloat => helpers.gt,
        OpCode::Gte | OpCode::GteFloat => helpers.gte,
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
}

fn emit_int_compare(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
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

    // Check tag for src1
    asm.mov_reg_reg(Reg::R10, Reg::Rax);
    asm.push(Reg::Rcx);
    asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R10, Reg::Rcx);
    asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
    asm.pop(Reg::Rcx);
    let p_fallback1 = asm.jmp_cond(crate::assembler::Cond::NotEqual);

    // Check tag for src2
    asm.mov_reg_reg(Reg::R10, Reg::R11);
    asm.push(Reg::Rcx);
    asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R10, Reg::Rcx);
    asm.cmp_reg_reg(Reg::R10, crate::registers::REG_INT_TAG);
    asm.pop(Reg::Rcx);
    let p_fallback2 = asm.jmp_cond(crate::assembler::Cond::NotEqual);

    // Both are inline integers! We can compare them inline.
    asm.shl_reg_imm8(Reg::Rax, 16);
    asm.shl_reg_imm8(Reg::R11, 16);

    asm.cmp_reg_reg(Reg::Rax, Reg::R11);

    let cond = match op {
        OpCode::LtInt => Cond::Less,
        OpCode::GtInt => Cond::Greater,
        OpCode::LteInt => Cond::LessEqual,
        OpCode::GteInt => Cond::GreaterEqual,
        OpCode::EqInt => Cond::Equal,
        OpCode::NeqInt => Cond::NotEqual,
        _ => unreachable!(),
    };

    let jmp_true_patch = asm.jmp_cond(cond);

    let false_val = 0x7FFA_0000_0000_0000u64;
    asm.mov_reg_imm64(Reg::Rax, false_val);
    let jmp_end_patch = asm.jmp_near();

    let true_pos = asm.current_offset();
    let disp_true = (true_pos as i32 - (jmp_true_patch as i32 + 4)) as u32;
    asm.patch_u32(jmp_true_patch, disp_true);

    let true_val = 0x7FFB_0000_0000_0000u64;
    asm.mov_reg_imm64(Reg::Rax, true_val);

    let end_pos = asm.current_offset();
    let disp_end = (end_pos as i32 - (jmp_end_patch as i32 + 4)) as u32;
    asm.patch_u32(jmp_end_patch, disp_end);

    emit_store(asm, Reg::Rax, first_reg, regmap);
    let p_skip_ffi = asm.jmp_near();

    // Slow path: FFI helper
    let slow = asm.current_offset();
    let disp_fallback1 = (slow as i32 - (p_fallback1 as i32 + 4)) as u32;
    asm.patch_u32(p_fallback1, disp_fallback1);
    let disp_fallback2 = (slow as i32 - (p_fallback2 as i32 + 4)) as u32;
    asm.patch_u32(p_fallback2, disp_fallback2);

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
        OpCode::EqInt => helpers.eq,
        OpCode::NeqInt => helpers.neq,
        OpCode::LtInt => helpers.lt,
        OpCode::LteInt => helpers.lte,
        OpCode::GtInt => helpers.gt,
        OpCode::GteInt => helpers.gte,
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

    let final_pos = asm.current_offset();
    let disp_skip = (final_pos as i32 - (p_skip_ffi as i32 + 4)) as u32;
    asm.patch_u32(p_skip_ffi, disp_skip);
}
