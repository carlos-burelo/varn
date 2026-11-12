use super::lower::{lower_type, NEVER_TY};
use super::resolve::NameResolver;
use vn_core::{AtomInterner, TypeKind};
use vn_sem::types::{CheckerTyTable, Type};
use vn_tir::{BackendTy, DynReason, TyTable};

pub(super) fn lower_union(
    members: vn_sem::types::TyListId,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    let member_ids = table.get_list(members);
    let is_null = |id: &vn_sem::types::CheckerTyId| {
        matches!(
            table.get(*id),
            TypeKind::Primitive(vn_core::LangPrimitive::Null)
        )
    };
    let non_null: Vec<&vn_sem::types::CheckerTyId> =
        member_ids.iter().filter(|id| !is_null(id)).collect();

    fn common_base(
        non_null: &[&vn_sem::types::CheckerTyId],
        table: &CheckerTyTable,
        interner: &AtomInterner,
        tt: &mut TyTable,
        names: &dyn NameResolver,
    ) -> Option<BackendTy> {
        let mut iter = non_null
            .iter()
            .map(|id| lower_type(&Type::resolved(**id), table, interner, tt, names));
        let first = iter.next()?;
        if matches!(first, BackendTy::Dynamic(_)) {
            return None;
        }
        iter.all(|t| t == first).then_some(first)
    }

    if non_null.len() == member_ids.len() {
        if let Some(base) = common_base(&non_null, table, interner, tt, names) {
            return base;
        }
        return BackendTy::Dynamic(DynReason::Union);
    }
    match non_null.as_slice() {
        [] => BackendTy::Nullable(NEVER_TY),
        [only] => {
            let inner = lower_type(&Type::resolved(**only), table, interner, tt, names);
            BackendTy::Nullable(tt.intern(inner))
        }
        _ => match common_base(&non_null, table, interner, tt, names) {
            Some(base) => BackendTy::Nullable(tt.intern(base)),
            None => BackendTy::Dynamic(DynReason::Union),
        },
    }
}
