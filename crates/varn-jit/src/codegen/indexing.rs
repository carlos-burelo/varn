use varn_core::OpCode;

use crate::assembler::{Cond, Reg};
use crate::regalloc::{emit_load, emit_store};
use crate::registers::{ARG_EXEC_CTX, REG_INT_TAG};

use super::ffi::{emit_ffi_call, FfiArg, FfiCallSpec};
use super::CodegenCtx;

pub(crate) fn emit_indexing(
    ctx: &mut CodegenCtx,
    op: OpCode,
    first_reg: usize,
) -> Result<(), String> {
    match op {
        OpCode::GetIndex | OpCode::ArrayGetIndex => emit_get_index(ctx, first_reg),
        OpCode::SetIndex | OpCode::ArraySetIndex => emit_set_index(ctx, first_reg),
        OpCode::Typeof => emit_typeof(ctx, first_reg),
        OpCode::Instanceof => emit_instanceof(ctx, first_reg),
        _ => unreachable!("emit_indexing called with {:?}", op),
    }
    Ok(())
}

fn emit_get_index(ctx: &mut CodegenCtx, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let obj_reg = (w1 >> 8) as usize;
    let key_reg = (w1 & 0xFF) as usize;

    let lay = ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;

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

        // Receiver must be a heap reference.
        let heap_mask = varn_types::vm_value::SIGN
            | varn_types::vm_value::QNAN
            | varn_types::vm_value::MASK_TAG;
        let heap_expect = varn_types::vm_value::SIGN
            | varn_types::vm_value::QNAN
            | varn_types::vm_value::TAG_PTR;
        asm.mov_reg_reg(Reg::R10, Reg::Rax);
        asm.mov_reg_imm64(Reg::Rcx, heap_mask);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.mov_reg_imm64(Reg::Rcx, heap_expect);
        asm.cmp_reg_reg(Reg::R10, Reg::Rcx);
        slow.push(asm.jmp_cond(Cond::NotEqual));

        // Key must be an int.
        asm.mov_reg_reg(Reg::R10, Reg::R11);
        asm.mov_reg_imm64(Reg::Rcx, 0x7FFF_0000_0000_0000u64);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.cmp_reg_reg(Reg::R10, REG_INT_TAG);
        slow.push(asm.jmp_cond(Cond::NotEqual));

        // slot = <generation base>.ptr + raw_idx * slot_size. Bit 31 of the
        // heap index picks the generation: set = old gen (`objects`),
        // clear = nursery.
        asm.mov_reg_imm64(Reg::Rcx, 0xFFFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::Rcx);
        asm.mov_reg_mem(Reg::Rcx, ARG_EXEC_CTX, heap_off as i32);
        asm.mov_reg_imm64(Reg::R10, 0x8000_0000u64);
        asm.test_reg_reg(Reg::Rax, Reg::R10);
        let p_old = asm.jmp_cond(Cond::NotEqual);
        // Nursery
        asm.mov_reg_mem(
            Reg::Rcx,
            Reg::Rcx,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let p_base_done = asm.jmp_near();
        // Old gen: strip the flag bit.
        let old_pos = asm.current_offset();
        asm.patch_u32(p_old, (old_pos as i32 - (p_old as i32 + 4)) as u32);
        asm.mov_reg_imm64(Reg::R10, 0x7FFF_FFFFu64);
        asm.and_reg_reg(Reg::Rax, Reg::R10);
        asm.mov_reg_mem(
            Reg::Rcx,
            Reg::Rcx,
            (lay.slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let base_pos = asm.current_offset();
        asm.patch_u32(p_base_done, (base_pos as i32 - (p_base_done as i32 + 4)) as u32);

        asm.mov_reg_imm64(Reg::R10, lay.slot_size as u64);
        asm.imul_reg_reg(Reg::Rax, Reg::R10);
        asm.add_reg_reg(Reg::Rax, Reg::Rcx);

        // The slot must hold an Array.
        asm.mov_reg_mem(Reg::R10, Reg::Rax, 0);
        asm.mov_reg_imm64(Reg::Rcx, 0xFF);
        asm.and_reg_reg(Reg::R10, Reg::Rcx);
        asm.mov_reg_imm64(Reg::Rcx, lay.array_tag as u64);
        asm.cmp_reg_reg(Reg::R10, Reg::Rcx);
        slow.push(asm.jmp_cond(Cond::NotEqual));

        // Element vec's words live at payload RcBox + 16.
        asm.mov_reg_mem(Reg::Rax, Reg::Rax, lay.payload_off as i32);

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

    let slow_pos = ctx.asm.current_offset();
    for p in slow {
        ctx.asm.patch_u32(p, (slow_pos as i32 - (p as i32 + 4)) as u32);
    }

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
