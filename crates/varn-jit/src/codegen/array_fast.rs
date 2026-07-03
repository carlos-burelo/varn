//! Shared building blocks for the inline array fast paths (read, write,
//! length). Each resolves a heap-tagged `VmValue` down to its element `Vec`
//! without any FFI call, branching to a caller-supplied slow path for
//! anything the fast path does not handle.

use crate::assembler::{Assembler, Cond, Reg};
use crate::registers::ARG_EXEC_CTX;
use crate::JitArrayLayout;

/// Emit the receiver-is-heap-array check and leave the array's element-`Vec`
/// base pointer in `Rax`. `obj` must already be loaded in `Rax`; `Rcx`,
/// `R10` are clobbered. Returns the jump-patch offsets that must target the
/// slow path.
///
/// When `nursery_only` is set, an old-generation parent also diverts to the
/// slow path (used by the store path, which would otherwise skip the
/// generational write barrier).
pub(crate) fn emit_resolve_array_payload(
    asm: &mut Assembler,
    lay: &JitArrayLayout,
    heap_off: usize,
    nursery_only: bool,
    slow: &mut Vec<usize>,
) {
    let heap_mask =
        varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG;
    let heap_expect =
        varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::TAG_PTR;
    asm.mov_reg_reg(Reg::R10, Reg::Rax);
    asm.mov_reg_imm64(Reg::Rcx, heap_mask);
    asm.and_reg_reg(Reg::R10, Reg::Rcx);
    asm.mov_reg_imm64(Reg::Rcx, heap_expect);
    asm.cmp_reg_reg(Reg::R10, Reg::Rcx);
    slow.push(asm.jmp_cond(Cond::NotEqual));

    // slot = <generation base>.ptr + raw_idx * slot_size. Bit 31 of the heap
    // index selects the generation: set = old gen (`objects`), clear =
    // nursery.
    asm.mov_reg_imm64(Reg::Rcx, 0xFFFF_FFFFu64);
    asm.and_reg_reg(Reg::Rax, Reg::Rcx);
    asm.mov_reg_mem(Reg::Rcx, ARG_EXEC_CTX, heap_off as i32);

    if nursery_only {
        asm.mov_reg_imm64(Reg::R10, 0x8000_0000u64);
        asm.test_reg_reg(Reg::Rax, Reg::R10);
        slow.push(asm.jmp_cond(Cond::NotEqual));
        asm.mov_reg_mem(
            Reg::Rcx,
            Reg::Rcx,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
    } else {
        asm.mov_reg_imm64(Reg::R10, 0x8000_0000u64);
        asm.test_reg_reg(Reg::Rax, Reg::R10);
        let p_old = asm.jmp_cond(Cond::NotEqual);
        asm.mov_reg_mem(
            Reg::Rcx,
            Reg::Rcx,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let p_base_done = asm.jmp_near();
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
    }

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

    // Element `Vec`'s words live at payload RcBox + 16.
    asm.mov_reg_mem(Reg::Rax, Reg::Rax, lay.payload_off as i32);
}

/// Patch every offset in `slow` to point at the current assembler position.
pub(crate) fn patch_all(asm: &mut Assembler, slow: Vec<usize>) {
    let pos = asm.current_offset();
    for p in slow {
        asm.patch_u32(p, (pos as i32 - (p as i32 + 4)) as u32);
    }
}
