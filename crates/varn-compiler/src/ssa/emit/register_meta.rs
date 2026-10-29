use super::super::ir::SsaFunc;
use varn_tir::{BackendTy, DynReason};

pub(super) fn derive_register_meta(
    ssa: &SsaFunc,
    reg: &[u8],
    register_count: u16,
    param_kinds: &[varn_types::register_meta::SlotKind],
) -> Vec<varn_types::register_meta::RegisterMeta> {
    use varn_types::register_meta::{RegisterMeta, SlotKind};
    let n = register_count as usize;
    let mut kinds: Vec<Option<SlotKind>> = vec![None; n];

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

pub(crate) fn slot_kind_of(ty: BackendTy) -> varn_types::register_meta::SlotKind {
    use varn_types::register_meta::SlotKind;
    match ty {
        BackendTy::Literal(_) => slot_kind_of(ty.representation()),
        BackendTy::Int => SlotKind::Int,
        BackendTy::Float => SlotKind::Float,
        BackendTy::Bool => SlotKind::Bool,
        BackendTy::Str => SlotKind::Str,

        BackendTy::Class(_)
        | BackendTy::Array(_)
        | BackendTy::Char
        | BackendTy::Bytes
        | BackendTy::Enum(_)
        | BackendTy::Fn(_)
        | BackendTy::Map(..)
        | BackendTy::Dict(_)
        | BackendTy::Set(_)
        | BackendTy::Tuple(_)
        | BackendTy::Dynamic(DynReason::Runtime) => SlotKind::Ref,

        BackendTy::Nullable(_)
        | BackendTy::Decimal
        | BackendTy::BigInt
        | BackendTy::Void
        | BackendTy::Never
        | BackendTy::Dynamic(_) => SlotKind::Dynamic,
    }
}
