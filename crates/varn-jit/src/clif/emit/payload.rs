//! Runtime-helper calls and the inline walks from a boxed value to its
//! payload.

use super::*;

/// Indirect call to a template-JIT runtime helper
/// (`extern "C" fn(exec_ctx, VmValue…) -> VmValue`). The admitted helpers
/// never allocate on the VM heap (no GC can run under a clif frame) and
/// raise VM errors by longjmp'ing to the outer setjmp, exactly like the
/// template's slow paths.
/// Normalize a RAW function's return value to boxed `VmValue` bits.
///
/// An `int`-returning raw yields an unboxed i64 payload — UNCONDITIONALLY.
/// Every arm of `emit_return_value`'s `SlotKind::Int` case produces one: an
/// `Int` register is already a payload, a boxed one goes through `use_int`,
/// and a float one converts and wraps. Every other return kind is boxed by
/// construction and passes straight through.
///
/// This used to re-tag only when the high bits were clear, on the theory that
/// a set NaN-box tag meant the value was already boxed. That test cannot tell
/// a boxed value from a NEGATIVE payload — `-3` is `0xFFFF_FFFF_FFFF_FFFD`,
/// whose high bits are all set — so every negative `int` return escaped
/// untagged and decoded as null. `function sub(a: int, b: int): int` returned
/// null for `sub(1, 4)`. Pinned by tests/59-clif-negative-int.vn.
///
/// Shared by `build_wrapper` and by the direct clif→clif call site: the two
/// consume the same raw entry and must decode its result identically.
pub(in crate::clif) fn retag_raw_return(
    b: &mut FunctionBuilder,
    raw_res: cranelift_codegen::ir::Value,
    return_kind: SlotKind,
) -> cranelift_codegen::ir::Value {
    match return_kind {
        SlotKind::Int => box_int(b, raw_res),
        SlotKind::Float => box_f64(b, raw_res),
        SlotKind::Bool => box_bool(b, raw_res),
        _ => raw_res,
    }
}

thread_local! {
    /// Set when the lowering emits a call to a helper the VM left at address 0
    /// (its body is still a fase-A tripwire). `try_compile` reads it and bails
    /// the whole function instead of emitting a call to `unreachable!`/null.
    static DISABLED_HELPER_HIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn reset_disabled_helper_hit() {
    DISABLED_HELPER_HIT.with(|c| c.set(false));
}

pub(crate) fn disabled_helper_hit() -> bool {
    DISABLED_HELPER_HIT.with(|c| c.get())
}

#[inline]
fn note_if_disabled(helper: usize) {
    if helper == 0 {
        DISABLED_HELPER_HIT.with(|c| c.set(true));
    }
}

pub(in crate::clif) fn call_helper(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    args: &[cranelift_codegen::ir::Value],
) -> cranelift_codegen::ir::Value {
    note_if_disabled(helper);
    let mut sig = Signature::new(cc);
    for _ in 0..args.len() {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));
    let sig_ref = b.import_signature(sig);
    let ptr = b.ins().iconst(types::I64, helper as i64);
    let call = b.ins().call_indirect(sig_ref, ptr, args);
    b.inst_results(call)[0]
}

/// Like [`call_helper`] but for a `-> ()` helper (`gc_safepoint`,
/// `array_push`, `set_fixed_field`).
pub(in crate::clif) fn call_helper_void(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    args: &[cranelift_codegen::ir::Value],
) {
    note_if_disabled(helper);
    let mut sig = Signature::new(cc);
    for _ in 0..args.len() {
        sig.params.push(AbiParam::new(types::I64));
    }
    let sig_ref = b.import_signature(sig);
    let ptr = b.ins().iconst(types::I64, helper as i64);
    b.ins().call_indirect(sig_ref, ptr, args);
}
/// Resolve a boxed receiver down to its array payload pointer (the three
/// `Vec<VmValue>` words live at payload+16). Any rejection — not a heap
/// value, or a slot that is not an array — branches to `slow`; on return the
/// builder is positioned in a fresh block where the payload is valid.
pub(in crate::clif) fn emit_array_payload(
    b: &mut FunctionBuilder,
    obj: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
    slow: cranelift_codegen::ir::Block,
) -> cranelift_codegen::ir::Value {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (b.ins().iconst(types::I64, HEAP_KIND), obj)
    };
    let tag = b.ins().band_imm_u(obj_tag, KIND_MASK);
    let is_heap = b.ins().icmp_imm_u(IntCC::Equal, tag, HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    let slot = obj_payload;
    let tagb = b.ins().uload8(types::I64, m, slot, lay.kind_off as i32);
    let is_arr = b.ins().icmp_imm_u(IntCC::Equal, tagb, lay.array_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_arr, ok, &[], slow, &[]);
    b.switch_to_block(ok);
    b.ins().load(types::I64, m, slot, lay.payload_off as i32)
}

/// The `ArrayRepr` discriminant (0 = `Boxed`, 1 = `I64`, 2 = `F64`) of an
/// already-resolved payload, zero-extended to `I64`.
///
/// Read at every element access rather than folded into
/// [`emit_array_payload`]: the resolve can be hoisted into a loop cache
/// (see [`cached_payload`]), but an array's repr changes *under* that cached
/// pointer — an empty array specializes on its first push, a typed array
/// migrates back to `Boxed` on a mismatched write. Both swap the contents of
/// the same `ArrayRepr` cell, so the cached pointer stays valid while the tag
/// under it does not.
///
/// This load DEREFERENCES `payload`, so it must be plain `trusted()` — NOT
/// `readonly`/`can_move`. `can_move` would let the mid-end speculate the deref
/// above the resolve's `is_arr` guard, reading `[payload + disc_off]` for a
/// non-array receiver (bogus payload) → segfault. The element loads keyed off
/// this discriminant use `trusted()` for the same reason.
pub(in crate::clif) fn array_disc(
    b: &mut FunctionBuilder,
    payload: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
) -> cranelift_codegen::ir::Value {
    b.ins().uload8(
        types::I64,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        payload,
        lay.disc_off as i32,
    )
}

/// A boxed `VmValue`'s payload word IS the int — extract it.
pub(in crate::clif) fn unbox_int(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    if b.func.dfg.value_type(v) == types::I128 {
        let (_tag, payload) = b.ins().isplit(v);
        payload
    } else {
        v
    }
}

/// Raise `integer overflow` if the CPU's overflow flag was set, otherwise yield `r`.
/// `exec_ctx` es siempre real (leaf param 0, frame-aware param 3): el raise lo
/// usa directo, sin getters ni placeholders.
pub(in crate::clif) fn guard_overflow(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    exec_ctx: cranelift_codegen::ir::Value,
    helper: usize,
    r: cranelift_codegen::ir::Value,
    overflow: cranelift_codegen::ir::Value,
    lhs: cranelift_codegen::ir::Value,
    rhs: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let raise = b.create_block();
    let cont = b.create_block();
    b.ins().brif(overflow, raise, &[], cont, &[]);

    b.switch_to_block(raise);
    let ba = box_int(b, lhs);
    let bb = box_int(b, rhs);
    let (a_tag, a_payload) = b.ins().isplit(ba);
    let (b_tag, b_payload) = b.ins().isplit(bb);
    call_helper_void(
        b,
        cc,
        helper,
        &[exec_ctx, a_tag, a_payload, b_tag, b_payload],
    );
    b.ins().jump(cont, &[]);

    b.switch_to_block(cont);
    r
}

/// Resolve boxed `obj` to the base of its instance payload, where compact
/// field offsets apply, branching to `invalid` unless it is an instance. The
/// payload shares the object's cell, so the base is a constant offset away.
pub(in crate::clif) fn emit_instance_payload(
    b: &mut FunctionBuilder,
    obj: cranelift_codegen::ir::Value,
    olay: &crate::JitObjectLayout,
    alay: &crate::JitArrayLayout,
    invalid: cranelift_codegen::ir::Block,
) -> cranelift_codegen::ir::Value {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (b.ins().iconst(types::I64, HEAP_KIND), obj)
    };
    let tag = b.ins().band_imm_u(obj_tag, KIND_MASK);
    let is_heap = b.ins().icmp_imm_u(IntCC::Equal, tag, HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], invalid, &[]);
    b.switch_to_block(chk);

    let kind = b
        .ins()
        .uload8(types::I64, m, obj_payload, alay.kind_off as i32);
    let is_inst = b
        .ins()
        .icmp_imm_u(IntCC::Equal, kind, olay.instance_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_inst, ok, &[], invalid, &[]);
    b.switch_to_block(ok);
    b.ins().iadd_imm_u(
        obj_payload,
        (olay.instance_data_off + olay.instance_values_off) as i64,
    )
}

/// Whether the heap object at `addr` is young: a store into it needs no
/// write barrier, so an inline store path may skip the helper that carries it.
pub(in crate::clif) fn is_young(
    b: &mut FunctionBuilder,
    addr: cranelift_codegen::ir::Value,
    alay: &crate::JitArrayLayout,
) -> cranelift_codegen::ir::Value {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let state = b.ins().uload8(types::I64, m, addr, alay.state_off as i32);
    b.ins()
        .icmp_imm_u(IntCC::Equal, state, alay.young_state as i64)
}
