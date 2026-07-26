//! Shared CLIF emission primitives for the lowering: value coercion
//! (box/unbox), the inline array-payload resolve chain and its loop cache,
//! and the runtime-helper call shim. Kept out of `lower.rs` so the
//! per-opcode walk there stays under the file-size governance limit; these
//! are the leaf builders that walk calls.

use std::collections::HashMap;

use cranelift_codegen::ir::{condcodes::IntCC, types, AbiParam, InstBuilder, MemFlags, Signature};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::SlotKind;
use varn_types::VmValue;

use super::kinds::{is_boxed_kind, K};

pub(super) const INT_TAG: i64 = 0x7FFC_0000_0000_0000u64 as i64;
pub(super) const MASK_48: i64 = 0x0000_FFFF_FFFF_FFFFu64 as i64;
pub(super) const HEAP_MASK: i64 =
    (varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG)
        as i64;
pub(super) const HEAP_EXPECT: i64 =
    (varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::TAG_PTR)
        as i64;

/// `site` is the buffer offset of the rel32 field; `target` the buffer
/// offset the call must reach.
pub(super) fn patch_rel32(buf: &mut [u8], site: usize, target: usize) {
    let disp = target as i64 - (site as i64 + 4);
    let disp = i32::try_from(disp).expect("clif: rel32 out of range");
    buf[site..site + 4].copy_from_slice(&disp.to_le_bytes());
}

pub(super) fn def_const(b: &mut FunctionBuilder, vars: &[Variable], reg: usize, v: i64) {
    let c = b.ins().iconst(types::I64, v);
    b.def_var(vars[reg], c);
}

/// Read a register as an unboxed int. `Int` vars are already raw; `Boxed`
/// vars coerce via the i48 sign-extend — bit-identical to the
/// interpreter's typed-op read of the payload.
pub(super) fn use_int(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    match state[r] {
        K::Int => Ok(b.use_var(vars[r])),
        k if is_boxed_kind(k) => {
            let v = b.use_var(vars[r]);
            let s = b.ins().ishl_imm(v, 16);
            Ok(b.ins().sshr_imm(s, 16))
        }
        k => Err(format!("clif: int use of {k:?} register")),
    }
}

/// Read a register as boxed VmValue bits (heap receivers, call args).
pub(super) fn use_boxed(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    if is_boxed_kind(state[r]) {
        Ok(b.use_var(vars[r]))
    } else {
        Err(format!("clif: boxed use of {:?} register r{r}", state[r]))
    }
}

/// The value a `Return src` yields, coerced to the raw function's return
/// convention: an int return stays an unboxed i48 payload (the wrapper and
/// the clif→clif fast call re-tag it); every other return kind yields boxed
/// VmValue bits that the wrapper passes through. A reachable `null` (a
/// constructor's implicit `return null`) short-circuits to the null bits.
pub(super) fn emit_return_value(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    return_kind: SlotKind,
    src: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    if state[src] == K::Poison {
        return Ok(b.ins().iconst(types::I64, VmValue::null().0 as i64));
    }
    Ok(match return_kind {
        SlotKind::Int => match state[src] {
            K::Int => b.use_var(vars[src]),
            K::Boxed => use_int(b, vars, state, src)?,
            k => return Err(format!("clif: unproven int return ({k:?})")),
        },
        // A float return: an F64 register boxes (with NaN canonicalization);
        // an int coerces int→float then boxes; already-boxed float bits pass
        // through.
        SlotKind::Float => match state[src] {
            K::Float => {
                let f = b.use_var(vars[src]);
                box_f64(b, f)
            }
            K::Int => {
                let iv = b.use_var(vars[src]);
                let f = b.ins().fcvt_from_sint(types::F64, iv);
                box_f64(b, f)
            }
            k if is_boxed_kind(k) => b.use_var(vars[src]),
            k => return Err(format!("clif: unproven float return ({k:?})")),
        },
        // string/ref: already boxed bits, passed through by the wrapper.
        SlotKind::Str | SlotKind::Ref => {
            if is_boxed_kind(state[src]) {
                b.use_var(vars[src])
            } else {
                return Err(format!("clif: non-boxed heap return ({:?})", state[src]));
            }
        }
        SlotKind::Bool => match state[src] {
            K::Bool => {
                let raw = b.use_var(vars[src]);
                box_bool(b, raw)
            }
            k if is_boxed_kind(k) => b.use_var(vars[src]),
            k => return Err(format!("clif: unproven bool return ({k:?})")),
        },
        // Dynamic (`any`): box whatever representation the value holds.
        SlotKind::Dynamic => match state[src] {
            K::Int => {
                let raw = b.use_var(vars[src]);
                box_int(b, raw)
            }
            K::Bool => {
                let raw = b.use_var(vars[src]);
                box_bool(b, raw)
            }
            k if is_boxed_kind(k) => b.use_var(vars[src]),
            k => return Err(format!("clif: unproven dynamic return ({k:?})")),
        },
    })
}

/// Whether the function contains any array opcode. Functions that both
/// call and touch arrays are rejected: a payload pointer cached across a
/// loop body would dangle if the call's fallback triggers a GC.
pub(super) fn code_has_array_ops(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
) -> Result<bool, String> {
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        if matches!(
            OpCode::from_u8(code[ip] as u8),
            Some(OpCode::ArrayGetIndex | OpCode::ArraySetIndex | OpCode::ArrayLength)
        ) {
            return Ok(true);
        }
        ip += info.len;
    }
    Ok(false)
}

/// Re-tag an unboxed int as a VmValue.
pub(super) fn box_int(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let m = b.ins().band_imm(v, MASK_48);
    b.ins().bor_imm(m, INT_TAG)
}

/// Box an unboxed 0/1 bool as a VmValue: `false` = 0x7FFA…, `true` = 0x7FFB…
/// (`0x7FFA_0000_0000_0000 | (v << 48)`, valid because v ∈ {0,1}).
pub(super) fn box_bool(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let shifted = b.ins().ishl_imm(v, 48);
    b.ins().bor_imm(shifted, 0x7FFA_0000_0000_0000u64 as i64)
}

/// Unbox a boxed bool VmValue (TAG_TRUE=0x7FFB…, TAG_FALSE=0x7FFA…) to 0/1:
/// the two tags differ only in bit 48, so `(v >> 48) & 1`.
pub(super) fn unbox_bool(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let s = b.ins().ushr_imm(v, 48);
    b.ins().band_imm(s, 1)
}

/// Box an unboxed `f64` as a VmValue, replicating `VmValue::from_f64`: a
/// float is stored as its raw bits EXCEPT a quiet-NaN-range result
/// (`bits & QNAN == QNAN`) canonicalizes to `null`. This keeps a native float
/// result byte-identical to the interpreter, which routes every float op
/// through `from_f64`.
pub(super) fn box_f64(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let bits = b.ins().bitcast(types::I64, MemFlags::new(), v);
    let q = b.ins().band_imm(bits, varn_types::vm_value::QNAN as i64);
    let is_qnan = b.ins().icmp_imm(IntCC::Equal, q, varn_types::vm_value::QNAN as i64);
    let null = b.ins().iconst(types::I64, VmValue::null().0 as i64);
    b.ins().select(is_qnan, null, bits)
}

/// Unbox a boxed float VmValue to a raw `f64` — a pure bitcast, since a float
/// VmValue's bits ARE the `f64` (canonical NaN-boxing).
pub(super) fn unbox_f64(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    b.ins().bitcast(types::F64, MemFlags::new(), v)
}

/// Mask isolating a VmValue's QNAN+tag bits, for an int-tag test.
pub(super) const INT_CHECK_MASK: i64 =
    (varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG) as i64;

/// Unbox a VmValue a float slot may hold as EITHER float bits OR an int
/// VmValue — the latter arises when a widening int argument is passed to a
/// float parameter (`takesFloat(5)`), where the caller boxes the int and the
/// callee must coerce, exactly as the interpreter's `to_f64_val` does. An
/// int-tagged value sign-extends its i48 payload and `fcvt`s; anything else is
/// reinterpreted as its f64 bits.
pub(super) fn unbox_f64_coerce(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let masked = b.ins().band_imm(v, INT_CHECK_MASK);
    let is_int = b.ins().icmp_imm(IntCC::Equal, masked, INT_TAG);
    let ext = wrap_i48(b, v);
    let from_int = b.ins().fcvt_from_sint(types::F64, ext);
    let from_bits = b.ins().bitcast(types::F64, MemFlags::new(), v);
    b.ins().select(is_int, from_int, from_bits)
}

/// Read a register as a raw `f64`: a `Float` var is already `F64`; an `Int`
/// var coerces via `fcvt_from_sint` (the interpreter's `to_f64_val` does the
/// same int→float widening in a mixed float op).
pub(super) fn use_f64(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> Result<cranelift_codegen::ir::Value, String> {
    match state[r] {
        K::Float => Ok(b.use_var(vars[r])),
        K::Int => {
            let iv = b.use_var(vars[r]);
            Ok(b.ins().fcvt_from_sint(types::F64, iv))
        }
        k => Err(format!("clif: f64 use of {k:?} register")),
    }
}

/// Whether register `r` is float-typed (its Variable is declared `F64`).
pub(super) fn meta_is_float(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    meta.get(r).map_or(false, |m| m.kind == SlotKind::Float)
}

/// Read a register as boxed VmValue bits regardless of its representation:
/// int → `box_int`, bool → `box_bool`, float → `box_f64`, already-boxed (or
/// any other tracked kind) → the raw bits. Callers pass registers whose
/// lattice kind the flow has already proven to hold a real value.
pub(super) fn box_or_pass(
    b: &mut FunctionBuilder,
    vars: &[Variable],
    state: &[K],
    r: usize,
) -> cranelift_codegen::ir::Value {
    let raw = b.use_var(vars[r]);
    match state[r] {
        K::Int => box_int(b, raw),
        K::Bool => box_bool(b, raw),
        K::Float => box_f64(b, raw),
        _ => raw,
    }
}

pub(super) fn state_meta_int(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    meta.get(r).map_or(false, |m| m.kind == SlotKind::Int)
}

/// Innermost loop region containing `ip` with a hoisted cache for `r`.
pub(super) fn find_cache(
    regions: &[(usize, usize, Vec<usize>)],
    cache_vars: &HashMap<(usize, usize), Variable>,
    ip: usize,
    r: usize,
) -> Option<Variable> {
    regions
        .iter()
        .filter(|(h, e, regs)| *h <= ip && ip < *e && regs.contains(&r))
        .min_by_key(|(h, e, _)| e - h)
        .map(|(h, _, _)| cache_vars[&(*h, r)])
}

/// Indirect call to a template-JIT runtime helper
/// (`extern "C" fn(exec_ctx, VmValue…) -> VmValue`). The admitted helpers
/// never allocate on the VM heap (no GC can run under a clif frame) and
/// raise VM errors by longjmp'ing to the outer setjmp, exactly like the
/// template's slow paths.
pub(super) fn call_helper(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    args: &[cranelift_codegen::ir::Value],
) -> cranelift_codegen::ir::Value {
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
pub(super) fn call_helper_void(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helper: usize,
    args: &[cranelift_codegen::ir::Value],
) {
    let mut sig = Signature::new(cc);
    for _ in 0..args.len() {
        sig.params.push(AbiParam::new(types::I64));
    }
    let sig_ref = b.import_signature(sig);
    let ptr = b.ins().iconst(types::I64, helper as i64);
    b.ins().call_indirect(sig_ref, ptr, args);
}

/// Payload via the loop cache when one exists for this access: cache != 0
/// short-circuits the whole guard walk (one test + branch, perfectly
/// predicted after iteration one); cache == 0 — or no cache — takes the
/// full resolve.
#[allow(clippy::too_many_arguments)]
pub(super) fn cached_payload(
    b: &mut FunctionBuilder,
    exec_ctx: cranelift_codegen::ir::Value,
    obj: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
    heap_off: usize,
    slow: cranelift_codegen::ir::Block,
    cache: Option<Variable>,
    readonly: bool,
) -> cranelift_codegen::ir::Value {
    match cache {
        Some(cv) => {
            let c = b.use_var(cv);
            let full = b.create_block();
            let ready = b.create_block();
            b.append_block_param(ready, types::I64);
            b.ins().brif(c, ready, &[c.into()], full, &[]);
            b.switch_to_block(full);
            let p = emit_array_payload(b, exec_ctx, obj, lay, heap_off, slow, false, readonly);
            b.ins().jump(ready, &[p.into()]);
            b.switch_to_block(ready);
            b.block_params(ready)[0]
        }
        None => emit_array_payload(b, exec_ctx, obj, lay, heap_off, slow, false, readonly),
    }
}

/// Resolve a boxed receiver down to its array payload pointer (the three
/// `Vec<VmValue>` words live at payload+16). Mirrors the template's
/// `emit_resolve_array_payload`: heap-tag check, generation select on bit
/// 31 of the index, slot tag check. Any rejection branches to `slow`; on
/// return the builder is positioned in a fresh block where the payload is
/// valid.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_array_payload(
    b: &mut FunctionBuilder,
    exec_ctx: cranelift_codegen::ir::Value,
    obj: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
    heap_off: usize,
    slow: cranelift_codegen::ir::Block,
    nursery_only: bool,
    readonly: bool,
) -> cranelift_codegen::ir::Value {
    // The chain down to the payload pointer is `readonly` FOR THE DURATION
    // OF ONE ROUTED ACTIVATION: heap indices only get rebound by a GC
    // move or slot reuse, and in the ALLOC-FREE subset no op can allocate
    // on the VM heap, so no collection can run underneath us — marking the
    // chain readonly lets Cranelift's mid-end hoist the whole resolve out
    // of loops. In an ALLOCATING function a back-edge safepoint CAN move
    // these indices and grow the old-gen Vec, so `readonly` is false there:
    // every access re-resolves from the (reloaded) receiver. The element
    // Vec's data/len words are never readonly — an out-of-bounds store's
    // append path reallocates them.
    let ro = if readonly {
        MemFlags::trusted().with_readonly().with_can_move()
    } else {
        MemFlags::trusted()
    };
    let tag = b.ins().band_imm(obj, HEAP_MASK);
    let is_heap = b.ins().icmp_imm(IntCC::Equal, tag, HEAP_EXPECT);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    let raw = b.ins().band_imm(obj, 0xFFFF_FFFF);
    let rc = b.ins().load(types::I64, ro, exec_ctx, heap_off as i32);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);

    let (base, idx) = if nursery_only {
        let cont = b.create_block();
        b.ins().brif(old_bit, slow, &[], cont, &[]);
        b.switch_to_block(cont);
        let base = b.ins().load(
            types::I64,
            ro,
            rc,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
        (base, raw)
    } else {
        let base_old = b.ins().load(
            types::I64,
            ro,
            rc,
            (lay.slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let base_nur = b.ins().load(
            types::I64,
            ro,
            rc,
            (lay.nursery_slots_vec_off + lay.slots_ptr_off) as i32,
        );
        let idx_old = b.ins().band_imm(raw, 0x7FFF_FFFF);
        let base = b.ins().select(old_bit, base_old, base_nur);
        let idx = b.ins().select(old_bit, idx_old, raw);
        (base, idx)
    };

    let byte_off = b.ins().imul_imm(idx, lay.slot_size as i64);
    let slot = b.ins().iadd(base, byte_off);
    let tagb = b.ins().uload8(types::I64, ro, slot, 0);
    let is_arr = b.ins().icmp_imm(IntCC::Equal, tagb, lay.array_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_arr, ok, &[], slow, &[]);
    b.switch_to_block(ok);
    b.ins().load(types::I64, ro, slot, lay.payload_off as i32)
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
/// above the resolve's `is_arr` guard, reading `[payload + 16]` for a
/// non-array receiver (bogus payload) → segfault. The element loads keyed off
/// this discriminant use `trusted()` for the same reason.
pub(super) fn array_disc(
    b: &mut FunctionBuilder,
    payload: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
) -> cranelift_codegen::ir::Value {
    b.ins().uload8(
        types::I64,
        MemFlags::trusted(),
        payload,
        (16 + lay.disc_off) as i32,
    )
}

/// `(v << 16) >> 16` — the canonical i48 wrap (varn_core::numeric), which
/// is also exactly the unboxing of an int-tagged VmValue payload.
pub(super) fn wrap_i48(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let s = b.ins().ishl_imm(v, 16);
    b.ins().sshr_imm(s, 16)
}
