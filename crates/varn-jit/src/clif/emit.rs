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

use super::kinds::{is_boxed_kind, K};

pub(super) const INT_TAG: i64 = 0x7FFC_0000_0000_0000u64 as i64;
pub(super) const MASK_48: i64 = 0x0000_FFFF_FFFF_FFFFu64 as i64;
pub(super) const HEAP_MASK: i64 =
    (varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::MASK_TAG)
        as i64;
pub(super) const HEAP_EXPECT: i64 =
    (varn_types::vm_value::SIGN | varn_types::vm_value::QNAN | varn_types::vm_value::TAG_PTR)
        as i64;

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
        Err(format!("clif: boxed use of {:?} register", state[r]))
    }
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
) -> cranelift_codegen::ir::Value {
    match cache {
        Some(cv) => {
            let c = b.use_var(cv);
            let full = b.create_block();
            let ready = b.create_block();
            b.append_block_param(ready, types::I64);
            b.ins().brif(c, ready, &[c.into()], full, &[]);
            b.switch_to_block(full);
            let p = emit_array_payload(b, exec_ctx, obj, lay, heap_off, slow, false);
            b.ins().jump(ready, &[p.into()]);
            b.switch_to_block(ready);
            b.block_params(ready)[0]
        }
        None => emit_array_payload(b, exec_ctx, obj, lay, heap_off, slow, false),
    }
}

/// Resolve a boxed receiver down to its array payload pointer (the three
/// `Vec<VmValue>` words live at payload+16). Mirrors the template's
/// `emit_resolve_array_payload`: heap-tag check, generation select on bit
/// 31 of the index, slot tag check. Any rejection branches to `slow`; on
/// return the builder is positioned in a fresh block where the payload is
/// valid.
pub(super) fn emit_array_payload(
    b: &mut FunctionBuilder,
    exec_ctx: cranelift_codegen::ir::Value,
    obj: cranelift_codegen::ir::Value,
    lay: &crate::JitArrayLayout,
    heap_off: usize,
    slow: cranelift_codegen::ir::Block,
    nursery_only: bool,
) -> cranelift_codegen::ir::Value {
    // The chain down to the payload pointer is `readonly` FOR THE DURATION
    // OF ONE ROUTED ACTIVATION: heap indices only get rebound by a GC
    // move or slot reuse, and no op in the routed subset (including the
    // admitted slow helpers) can allocate on the VM heap, so no collection
    // can run underneath us. Marking them readonly lets Cranelift's
    // mid-end hoist the whole resolve out of loops. The element Vec's
    // data/len words are NOT readonly — an out-of-bounds store's append
    // path reallocates them.
    let ro = MemFlags::trusted().with_readonly().with_can_move();
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

/// `(v << 16) >> 16` — the canonical i48 wrap (varn_core::numeric), which
/// is also exactly the unboxing of an int-tagged VmValue payload.
pub(super) fn wrap_i48(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let s = b.ins().ishl_imm(v, 16);
    b.ins().sshr_imm(s, 16)
}
