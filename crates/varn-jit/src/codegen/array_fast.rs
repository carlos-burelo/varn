//! Shared building blocks for the inline array fast paths (read, write,
//! length). Each resolves a heap-tagged `VmValue` down to its element `Vec`
//! without any FFI call, branching to a caller-supplied slow path for
//! anything the fast path does not handle.

use crate::assembler::{Assembler, Cond, Reg};
use crate::regalloc::{emit_load, RegMap};
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

/// `Boxed`-repr guard on an already-resolved payload pointer in `Rax`: the
/// `ArrayRepr` tag byte at `RcBox + 16 + disc_off` must be `0`. A typed
/// (`I64`/`F64`) repr holds raw numbers where the inline path expects
/// NaN-boxed `VmValue`s, so it diverts to the generic helper. `Rax` is left
/// untouched; `Rcx`, `R10` are clobbered.
///
/// This is deliberately NOT part of [`emit_resolve_array_payload`]: a loop
/// hoists that resolve into a register once (see [`emit_resolve_into_cache`]),
/// but an array's repr can change *inside* the loop — an empty array
/// specializes on its first push, a typed array migrates back to `Boxed` on a
/// mismatched write. Both keep the same `ArrayRepr` cell, so the cached
/// pointer stays valid while the tag under it does not. Re-reading the tag at
/// every use is what keeps a hoisted payload honest.
///
/// Length reads do not need this guard at all — `elems_len_off` lands on the
/// `Vec` length word of every variant, verified at startup by
/// `Heap::jit_array_layout`'s typed-repr probe.
pub(crate) fn emit_boxed_repr_guard(
    asm: &mut Assembler,
    lay: &JitArrayLayout,
    slow: &mut Vec<usize>,
) {
    asm.mov_reg_mem(Reg::R10, Reg::Rax, (16 + lay.disc_off) as i32);
    asm.mov_reg_imm64(Reg::Rcx, 0xFF);
    asm.and_reg_reg(Reg::R10, Reg::Rcx);
    asm.test_reg_reg(Reg::R10, Reg::R10);
    slow.push(asm.jmp_cond(Cond::NotEqual));
}

/// Patch every offset in `slow` to point at the current assembler position.
pub(crate) fn patch_all(asm: &mut Assembler, slow: Vec<usize>) {
    let pos = asm.current_offset();
    for p in slow {
        asm.patch_u32(p, (pos as i32 - (p as i32 + 4)) as u32);
    }
}

/// Resolve `obj_vreg`'s array-box pointer into `cache_reg`, or `0` if the
/// guard chain rejects it (not a heap array — e.g. a nullable array-typed
/// local holding `null`). Used both by a loop's preheader (first resolve)
/// and by its back-edge GC safepoint (re-resolve after a collection may
/// have moved the object — see `loop_hoist` module docs).
///
/// `0` is a safe sentinel: no live heap allocation is ever placed at
/// address 0. Call sites that consume `cache_reg` must `test`/`jz` on it
/// before trusting it — see `emit_cached_or` below. This keeps hoisting
/// sound even though the *type* guarantee behind `ArrayGetIndex`/
/// `ArrayLength` (checker proved the receiver statically Array) does NOT
/// also guarantee non-null: `TypeKind::Array(_)` is checked past
/// `non_nullified()` in `checker_annotations.rs`, so a `null`-valued
/// nullable-array receiver reaches this guard too.
pub(crate) fn emit_resolve_into_cache(
    asm: &mut Assembler,
    lay: &JitArrayLayout,
    heap_off: usize,
    regmap: &RegMap,
    obj_vreg: u8,
    cache_reg: Reg,
) {
    emit_load(asm, Reg::Rax, obj_vreg as usize, regmap);
    let mut slow: Vec<usize> = Vec::new();
    emit_resolve_array_payload(asm, lay, heap_off, false, &mut slow);
    asm.mov_reg_reg(cache_reg, Reg::Rax);
    let done = asm.jmp_near();
    patch_all(asm, slow);
    asm.mov_reg_imm64(cache_reg, 0);
    let end = asm.current_offset();
    asm.patch_u32(done, (end as i32 - (done as i32 + 4)) as u32);
}

/// At a hoisted `ArrayGetIndex`/`ArrayLength` site: if `cache_reg` holds a
/// valid resolved pointer, copy it into `Rax` and skip the guard chain;
/// otherwise fall through so the caller emits the normal, unhoisted
/// resolve. Invariance guarantees `cache_reg`'s validity is the same on
/// every iteration of one loop execution, so the branch is perfectly
/// predicted after iteration one.
pub(crate) fn emit_cached_or_fallthrough(asm: &mut Assembler, cache_reg: Reg) -> usize {
    asm.test_reg_reg(cache_reg, cache_reg);
    let invalid = asm.jmp_cond(Cond::Equal);
    asm.mov_reg_reg(Reg::Rax, cache_reg);
    let hit = asm.jmp_near();
    let fallback_pos = asm.current_offset();
    asm.patch_u32(invalid, (fallback_pos as i32 - (invalid as i32 + 4)) as u32);
    hit
}
