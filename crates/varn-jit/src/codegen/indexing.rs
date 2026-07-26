use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::{emit_load, emit_store};
use crate::registers::REG_INT_TAG;

use super::array_fast::{
    emit_boxed_repr_guard, emit_cached_or_fallthrough, emit_resolve_array_payload, patch_all,
};
use super::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use super::CodegenCtx;

/// Emit the "key in R11 must be an int" guard, pushing to the slow list.
fn emit_int_key_guard(asm: &mut crate::assembler::Assembler, slow: &mut Vec<usize>) {
    asm.mov_reg_reg(Reg::R10, Reg::R11);
    asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
    asm.and_reg_reg(Reg::R10, Reg::Rcx);
    asm.cmp_reg_reg(Reg::R10, REG_INT_TAG);
    slow.push(asm.jmp_cond(Cond::NotEqual));
}

pub(crate) fn emit_indexing(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::GetIndex | OpCode::ArrayGetIndex => emit_get_index(ctx, op, first_reg),
        OpCode::SetIndex | OpCode::ArraySetIndex => emit_set_index(ctx, first_reg),
        OpCode::Typeof => emit_typeof(ctx, first_reg),
        OpCode::Instanceof => emit_instanceof(ctx, first_reg),
        _ => unreachable!("emit_indexing called with {:?}", op),
    }
    Ok(())
}

fn emit_get_index(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let instr_start = ctx.ip - 1;
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let obj_reg = (w1 >> 8) as usize;
    let key_reg = (w1 & 0xFF) as usize;

    let lay = ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;

    // Only ArrayGetIndex is eligible: it is the opcode the checker proved
    // statically Array-typed (GetIndex covers everything the checker could
    // NOT narrow, so its receiver may not even be an array).
    let cache_reg = (op == OpCode::ArrayGetIndex)
        .then(|| {
            ctx.hoist_plans.iter().find(|p| p.contains(instr_start)).and_then(|p| {
                p.cache_reg_for(ctx.code, &ctx.proto.chunk.constants, obj_reg as u8, instr_start)
            })
        })
        .flatten();

    // Fast path: heap array + in-bounds int key resolve to a single chain
    // of loads (VmValue → heap slot → element vec → data[idx]), no FFI.
    // Everything else — non-array receivers, out-of-bounds (→ null),
    // string/range indexing — falls back to the generic helper.
    let mut slow: Vec<usize> = Vec::new();
    {
        let asm = &mut ctx.asm;
        let regmap = &ctx.regmap;

        emit_load(asm, Reg::Rax, obj_reg, regmap);
        emit_load(asm, Reg::R11, key_reg, regmap);

        emit_int_key_guard(asm, &mut slow);

        let cache_hit = cache_reg.map(|reg| emit_cached_or_fallthrough(asm, reg));
        emit_resolve_array_payload(asm, &lay, heap_off, false, &mut slow);
        if let Some(p) = cache_hit {
            let after = asm.current_offset();
            asm.patch_u32(p, (after as i32 - (p as i32 + 4)) as u32);
        }
        // After the cache merge: a hoisted payload can outlive the repr it
        // was resolved from, so the tag is checked on every read.
        emit_boxed_repr_guard(asm, &lay, &mut slow);

        // Untag the key (48-bit sign extend); unsigned bounds check also
        // rejects negative indices.
        asm.shl_reg_imm8(Reg::R11, 16);
        asm.sar_reg_imm8(Reg::R11, 16);
        asm.mov_reg_mem(Reg::R10, Reg::Rax, (16 + lay.elems_len_off) as i32);
        asm.cmp_reg_reg(Reg::R11, Reg::R10);
        slow.push(asm.jmp_cond(Cond::AboveEqual));

        asm.mov_reg_mem(Reg::Rax, Reg::Rax, (16 + lay.elems_ptr_off) as i32);
        asm.shl_reg_imm8(Reg::R11, 3);
        asm.add_reg_reg(Reg::Rax, Reg::R11);
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, 0);
        emit_store(asm, Reg::Rax, first_reg, regmap);
    }
    let done = ctx.asm.jmp_near();

    patch_all(&mut ctx.asm, slow);

    // The fast helper neither reads frame slots nor triggers GC on the
    // fast path, so allocated registers stay live across the call; the
    // slow path may grow the stack, hence the frame recompute.
    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.jit_array_get_fast,
            args: &[FfiArg::Vreg(obj_reg), FfiArg::Vreg(key_reg)],
            flush: false,
            dest: Some(first_reg),
            reload: false,
            recompute_frame: true,
        },
    );

    let end = ctx.asm.current_offset();
    ctx.asm
        .patch_u32(done, (end as i32 - (done as i32 + 4)) as u32);
}

fn emit_set_index(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let idx_reg = (w1 >> 8) as usize;
    let val_reg = (w1 & 0xFF) as usize;
    let obj_reg = first_reg;

    let lay = ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;

    // Fast path: in-bounds int store into a NURSERY array. Nursery parents
    // never need the write barrier (it only tracks old→nursery edges), so
    // the store is a plain memory write. Old-gen parents, out-of-bounds
    // stores (push/extend semantics) and non-array receivers fall back.
    let mut slow: Vec<usize> = Vec::new();
    {
        let asm = &mut ctx.asm;
        let regmap = &ctx.regmap;

        emit_load(asm, Reg::Rax, obj_reg, regmap);
        emit_load(asm, Reg::R11, idx_reg, regmap);

        emit_int_key_guard(asm, &mut slow);
        // nursery_only: old-gen parents divert so the write barrier runs.
        emit_resolve_array_payload(asm, &lay, heap_off, true, &mut slow);
        emit_boxed_repr_guard(asm, &lay, &mut slow);

        // Untag the key; unsigned bounds check (strictly below len — the
        // idx == len append case belongs to the helper).
        asm.shl_reg_imm8(Reg::R11, 16);
        asm.sar_reg_imm8(Reg::R11, 16);
        asm.mov_reg_mem(Reg::R10, Reg::Rax, (16 + lay.elems_len_off) as i32);
        asm.cmp_reg_reg(Reg::R11, Reg::R10);
        slow.push(asm.jmp_cond(Cond::AboveEqual));

        asm.mov_reg_mem(Reg::Rax, Reg::Rax, (16 + lay.elems_ptr_off) as i32);
        asm.shl_reg_imm8(Reg::R11, 3);
        asm.add_reg_reg(Reg::Rax, Reg::R11);
        emit_load(asm, Reg::R10, val_reg, regmap);
        asm.mov_mem_reg(Reg::Rax, 0, Reg::R10);
    }
    let done = ctx.asm.jmp_near();

    patch_all(&mut ctx.asm, slow);

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.jit_array_set_fast,
            args: &[
                FfiArg::Vreg(obj_reg),
                FfiArg::Vreg(idx_reg),
                FfiArg::Vreg(val_reg),
            ],
            flush: true,
            dest: None,
            reload: true,
            recompute_frame: true,
        },
    );

    let end = ctx.asm.current_offset();
    ctx.asm
        .patch_u32(done, (end as i32 - (done as i32 + 4)) as u32);
}

fn emit_typeof(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src = (w1 >> 8) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.typeof_val,
            args: &[FfiArg::Vreg(src)],
            flush: true,
            dest: Some(first_reg),
            reload: true,
            recompute_frame: true,
        },
    );
}

fn emit_instanceof(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    emit_ffi_call(
        &mut ctx.asm,
        &ctx.regmap,
        &FfiCallSpec {
            helper: ctx.helpers.instanceof,
            args: &[FfiArg::Vreg(src1), FfiArg::Vreg(src2)],
            flush: false,
            dest: Some(first_reg),
            reload: false,
            recompute_frame: true,
        },
    );
}
