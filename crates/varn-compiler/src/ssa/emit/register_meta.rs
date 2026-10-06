use super::super::ir::SsaFunc;

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

pub(crate) fn slot_kind_of(ty: crate::hir::HirType) -> varn_types::register_meta::SlotKind {
    use crate::hir::HirType;
    use varn_types::register_meta::SlotKind;
    match ty {
        HirType::Int => SlotKind::Int,
        HirType::Float => SlotKind::Float,
        HirType::Bool => SlotKind::Bool,
        HirType::Str => SlotKind::Str,

        HirType::Class(_)
        | HirType::Array(_)
        | HirType::Ref
        | HirType::Map(_, _)
        | HirType::Set(_) => SlotKind::Ref,

        HirType::Nullable(_) => SlotKind::Dynamic,
        HirType::Dynamic => SlotKind::Dynamic,
    }
}
