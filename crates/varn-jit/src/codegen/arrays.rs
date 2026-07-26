use varn_core::OpCode;

use crate::assembler::Reg;
use crate::regalloc::{emit_flush_all, emit_load, emit_reload_all_except, emit_store};
use crate::registers::{ARG_BASE, ARG_CLOSURE, ARG_CTX, ARG_EXEC_CTX};

use super::array_fast::emit_cached_or_fallthrough;
use super::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use super::CodegenCtx;

pub(crate) fn emit_arrays(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::BuildArray => emit_build_array(ctx, first_reg),
        OpCode::ArrayLength => emit_array_length(ctx, first_reg),
        OpCode::ArrayPush => emit_array_push(ctx, first_reg),
        OpCode::ArrayPop => emit_array_pop(ctx, first_reg),
        OpCode::ArrayExtend => emit_array_extend(ctx, first_reg),
        OpCode::BuildStr => emit_build_str(ctx, first_reg),
        _ => unreachable!("emit_arrays called with {:?}", op),
    }
    Ok(())
}

fn emit_build_array(ctx: &mut CodegenCtx, _first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let w2 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let dest = (w1 >> 8) as usize;
    let start_reg = (w1 & 0xFF) as usize;
    let count = (w2 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.build_array,
            args: &[FfiArg::Imm(start_reg as u64), FfiArg::Imm(count as u64)],
            flush: true,
            dest: Some(dest),
            reload: true,
            recompute_frame: false,
        },
    );
}

fn emit_array_length(ctx: &mut CodegenCtx, first_reg: usize) {
    let instr_start = ctx.ip - 1;
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    let lay = ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;

    let cache_reg = ctx.hoist_plans.iter().find(|p| p.contains(instr_start)).and_then(|p| {
        p.cache_reg_for(ctx.code, &ctx.proto.chunk.constants, src as u8, instr_start)
    });

    // Fast path: a heap array's length is a single Vec-len load, NaN-boxed
    // as an int. Non-array receivers (strings, etc.) fall back. No repr
    // guard: `elems_len_off` lands on the `Vec` length word of every
    // `ArrayRepr` variant (checked at startup by `Heap::jit_array_layout`),
    // so a typed array reads its length inline just like a boxed one.
    let mut slow: Vec<usize> = Vec::new();
    {
        let asm = &mut ctx.asm;
        let regmap = &ctx.regmap;

        emit_load(asm, Reg::Rax, src, regmap);

        let cache_hit = cache_reg.map(|reg| emit_cached_or_fallthrough(asm, reg));
        super::array_fast::emit_resolve_array_payload(asm, &lay, heap_off, false, &mut slow);
        if let Some(p) = cache_hit {
            let after = asm.current_offset();
            asm.patch_u32(p, (after as i32 - (p as i32 + 4)) as u32);
        }

        // len (payload+16 + elems_len_off) → NaN-boxed int.
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, (16 + lay.elems_len_off) as i32);
        asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
        emit_store(asm, Reg::Rax, first_reg, regmap);
    }
    let done = ctx.asm.jmp_near();

    super::array_fast::patch_all(&mut ctx.asm, slow);

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.array_length,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );

    let end = ctx.asm.current_offset();
    ctx.asm
        .patch_u32(done, (end as i32 - (done as i32 + 4)) as u32);
}

fn emit_array_push(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.array_push,
            args: &[FfiArg::Vreg(first_reg), FfiArg::Vreg(src)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

fn emit_array_pop(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.array_pop,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: false,
        },
    );
}

fn emit_array_extend(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.array_extend,
            args: &[FfiArg::Vreg(first_reg), FfiArg::Vreg(src)],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: false,
        },
    );
}

/// BuildStr builds its part list on the machine stack and passes a pointer,
/// so it stays outside `emit_ffi_call`.
fn emit_build_str(ctx: &mut CodegenCtx, first_reg: usize) {
    let asm = &mut ctx.asm;
    let code = ctx.code;
    let ip = &mut ctx.ip;
    let regmap = &ctx.regmap;
    let helpers = ctx.helpers;

    let w1 = code[*ip];
    let count = (w1 >> 8) as usize;
    *ip += 1;

    let original_ip = *ip;
    *ip += count;

    let mut reg_indices = Vec::with_capacity(count);
    for i in 0..count {
        let w_reg = code[original_ip + i];
        let r_idx = (w_reg >> 8) as usize;
        reg_indices.push(r_idx);
    }

    emit_flush_all(asm, regmap);

    asm.push(ARG_CTX);
    asm.push(ARG_CLOSURE);
    asm.push(ARG_BASE);
    asm.push(ARG_EXEC_CTX);

    let need_dummy = (regmap.used_phys.len() + count) % 2 == 0;
    if need_dummy {
        asm.push(Reg::Rax);
    }

    for &r_idx in reg_indices.iter().rev() {
        emit_load(asm, Reg::Rax, r_idx, regmap);
        asm.push(Reg::Rax);
    }

    asm.mov_reg_reg(ARG_CTX, ARG_EXEC_CTX);
    asm.mov_reg_reg(ARG_CLOSURE, Reg::Rsp);
    asm.mov_reg_imm64(ARG_BASE, count as u64);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, -32);

    asm.mov_reg_imm64(Reg::R10, helpers.build_str as u64);
    asm.call_reg(Reg::R10);

    #[cfg(target_os = "windows")]
    asm.add_reg_imm8(Reg::Rsp, 32);

    asm.mov_reg_reg(Reg::R11, Reg::Rax);

    asm.add_reg_imm32(Reg::Rsp, (count * 8) as i32);

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
