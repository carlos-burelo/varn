//! Inline nursery allocation for CLIF.
//!
//! Reading a nursery slot inline is already established — `clif/fields.rs`
//! and `clif/emit.rs` address `nursery_ptr + idx * slot_size` for property and
//! array fast paths, and `alloc::emit_gc_safepoint_check` already loads the
//! live-object count. This adds *writing* a fresh one.
//!
//! Sound because of three properties, all of which must hold:
//!
//! 1. **The backing store never moves.** `Nursery::new` reserves both Vecs to
//!    `NURSERY_CAPACITY` from birth, so no `push` can realloc under emitted
//!    code holding a slot address.
//! 2. **The bump is not a safepoint.** It cannot collect; a full nursery takes
//!    the `slow` block. Collection still happens only at back-edge safepoints
//!    and inside helpers.
//! 3. **`forwarding` tracks `objects`.** The minor collector indexes both by
//!    nursery index, so both lengths are bumped here. Bumping one is a silent
//!    out-of-bounds on the next collection.
//!
//! On (3): the new `forwarding` element also needs an explicit `None` write,
//! not just a length bump. `Nursery::collect` ends with `self.objects.clear();
//! self.forwarding.clear();` — `Vec::clear` truncates the length to 0 without
//! touching the backing bytes, so a slot the next epoch reuses can still hold
//! a stale `Some(idx)` from before the collection. Left alone, that would make
//! the collector treat a fresh nursery object as already-forwarded. So this
//! writes a probed `None` bit pattern into the new slot on every allocation
//! (`JitStrLayout::fwd_none_pattern`/`fwd_elem_size`) rather than relying on
//! `collect` to have zeroed anything — it doesn't.

use cranelift_codegen::ir::{condcodes::IntCC, types, Block, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;

use crate::JitHelpers;

pub(super) struct NurserySlot {
    /// Machine address of the freshly reserved `Option<HeapObj>` slot.
    pub addr: Value,
    /// Its nursery index, ready to be tagged into a heap `VmValue`.
    pub idx: Value,
}

/// Reserve one nursery slot, or branch to `slow` if the nursery is full.
pub(super) fn emit_nursery_alloc(
    b: &mut FunctionBuilder,
    helpers: &JitHelpers,
    exec_ctx: Value,
    slow: Block,
) -> NurserySlot {
    let alay = &helpers.array_layout;
    let slay = &helpers.str_layout;
    let m = MemFlags::trusted();

    // ExecCtx -> heap Rc -> RcBox base, the same walk the field fast paths do.
    let rcbox = b
        .ins()
        .load(types::I64, m, exec_ctx, helpers.heap_field_offset as i32);

    // 1. Capacity check. NOT the GC threshold: this is the hard bound past
    //    which `try_alloc` itself declines. Crossing the softer
    //    `nursery_threshold` is the back-edge safepoint's business.
    //    The length offset already lives on `JitHelpers` (the back-edge
    //    safepoint reads it); it is deliberately NOT duplicated onto
    //    `JitStrLayout`.
    let len = b
        .ins()
        .load(types::I64, m, rcbox, helpers.nursery_len_offset as i32);
    let has_room = b
        .ins()
        .icmp_imm(IntCC::UnsignedLessThan, len, slay.nursery_capacity as i64);
    let ok = b.create_block();
    b.ins().brif(has_room, ok, &[], slow, &[]);
    b.switch_to_block(ok);

    // 2. Bump both lengths. `forwarding`'s new entry must read as `None`;
    //    `Option<u32>` is 8 bytes with a niche, so its `None` pattern is
    //    whatever the probe captured — see step 3.
    let next = b.ins().iadd_imm(len, 1);
    b.ins()
        .store(m, next, rcbox, helpers.nursery_len_offset as i32);
    let fwd_len_off = slay.nursery_fwd_vec_off + 2 * std::mem::size_of::<usize>();
    b.ins().store(m, next, rcbox, fwd_len_off as i32);

    // 3. alloc_count, so JIT allocations are visible to `bench -v` and to the
    //    GC statistics rather than silently uncounted.
    let ac = b
        .ins()
        .load(types::I64, m, rcbox, slay.alloc_count_off as i32);
    let ac = b.ins().iadd_imm(ac, 1);
    b.ins().store(m, ac, rcbox, slay.alloc_count_off as i32);

    // 4. Slot address: nursery objects data pointer + len * slot_size.
    let data = b.ins().load(
        types::I64,
        m,
        rcbox,
        (alay.nursery_slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let off = b.ins().imul_imm(len, alay.slot_size as i64);
    let addr = b.ins().iadd(data, off);

    // 5. Write the new `forwarding` element as `None` — see the module doc:
    //    `collect` clears both Vecs without zeroing, so the byte range this
    //    bump just claimed can still hold a stale `Some(idx)` from a prior
    //    epoch. `fwd_elem_size` is asserted to be 8 at the layout probe (the
    //    only width this single 8-byte store is correct for).
    let fwd_data = b.ins().load(
        types::I64,
        m,
        rcbox,
        (slay.nursery_fwd_vec_off + alay.slots_ptr_off) as i32,
    );
    let fwd_off = b.ins().imul_imm(len, slay.fwd_elem_size as i64);
    let fwd_addr = b.ins().iadd(fwd_data, fwd_off);
    let none = b.ins().iconst(types::I64, slay.fwd_none_pattern as i64);
    b.ins().store(m, none, fwd_addr, 0);

    NurserySlot { addr, idx: len }
}
