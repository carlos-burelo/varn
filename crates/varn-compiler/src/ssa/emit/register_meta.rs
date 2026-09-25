//! Per-register slot kinds, derived from the checker-proven SSA types.

use super::super::ir::SsaFunc;

/// Per-register slot kinds from checker-proven SSA value types: the meet of
/// every value a register hosts, plus `Dynamic` for caller-written slots
/// (callee, params, `this`) and helper registers that host no SSA value
/// (scratch, call staging, null). Replaces the old opcode-walking
/// re-inference in `varn-regalloc`, which guessed back what the checker
/// already proved. `regalloc_post` re-permutes this when it coalesces.
pub(super) fn derive_register_meta(
    ssa: &SsaFunc,
    reg: &[u8],
    register_count: u16,
    param_kinds: &[varn_types::register_meta::SlotKind],
) -> Vec<varn_types::register_meta::RegisterMeta> {
    use varn_types::register_meta::{RegisterMeta, SlotKind};
    let n = register_count as usize;
    let mut kinds: Vec<Option<SlotKind>> = vec![None; n];
    // r0 is the callee/`this` staging slot the caller writes; keep it Dynamic
    // (it may host a heap ref during call staging, and the GC must flush it).
    // Params (at r1+i, matching the JIT's entry contract) carry their declared
    // kind: an immediate param (int/float/bool) is a proven fact the backend
    // uses — it skips the GC flush and, for float, routes to native f64 — while
    // a heap-ref param stays non-immediate and still flushes. The value meet
    // below downgrades any param register the allocator reuses for a
    // differently-typed value back to Dynamic.
    if n > 0 {
        kinds[0] = Some(SlotKind::Dynamic);
    }
    for (i, pk) in param_kinds.iter().enumerate() {
        if 1 + i < n {
            kinds[1 + i] = Some(*pk);
        }
    }
    for (vi, def) in ssa.values.iter().enumerate() {
        let Some(&r) = reg.get(vi) else { continue };
        let r = r as usize;
        if r >= n {
            continue;
        }
        let k = slot_kind_of(def.ty);
        kinds[r] = Some(match kinds[r] {
            None => k,
            Some(cur) if cur == k => cur,
            Some(_) => SlotKind::Dynamic,
        });
    }
    kinds
        .into_iter()
        .map(|k| RegisterMeta {
            kind: k.unwrap_or(SlotKind::Dynamic),
        })
        .collect()
}

pub(crate) fn slot_kind_of(ty: crate::hir::HirType) -> varn_types::register_meta::SlotKind {
    use crate::hir::HirType;
    use varn_types::register_meta::SlotKind;
    match ty {
        HirType::Int => SlotKind::Int,
        HirType::Float => SlotKind::Float,
        HirType::Bool => SlotKind::Bool,
        HirType::Str => SlotKind::Str,
        // Every heap-only shape collapses to `Ref` — the id was never read by
        // the backend.
        HirType::Class(_)
        | HirType::Array(_)
        | HirType::Ref
        | HirType::Map(_, _)
        | HirType::Set(_) => SlotKind::Ref,
        // A nullable of anything is boxed today (tag word says null-or-not);
        // the backend has no `Pair` kind yet.
        HirType::Nullable(_) => SlotKind::Dynamic,
        HirType::Dynamic => SlotKind::Dynamic,
    }
}
