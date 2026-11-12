use rustc_hash::FxHashMap;
use std::sync::Arc;
use vn_core::TypeKind;
use vn_sem::types::{CheckerTyTable, Type};

pub(crate) fn collect_type_inferences(
    expected: &Type,
    actual: &Type,
    params: &[Arc<str>],
    out: &mut FxHashMap<Arc<str>, Type>,
    table: &mut CheckerTyTable,
    interner: &vn_core::AtomInterner,
) {
    let expected_kind = table.get(expected.0);
    let expects_type_param = matches!(expected_kind, TypeKind::Named(name, _)
        if params.iter().any(|p| p.as_ref() == interner.resolve(name)));
    if !expects_type_param && let TypeKind::Union(members) = table.get(actual.0) {
        for member in table.get_list(members).to_vec() {
            collect_type_inferences(
                expected,
                &Type::resolved(member),
                params,
                out,
                table,
                interner,
            );
        }
        return;
    }
    match expected_kind {
        TypeKind::Named(name, _origin)
            if params.iter().any(|p| p.as_ref() == interner.resolve(name)) =>
        {
            let name_rc: Arc<str> = Arc::from(interner.resolve(name));
            let entry = out.entry(name_rc).or_insert(*actual);
            if entry != actual {
                *entry = Type::union(vec![*entry, *actual], table);
            }
        }
        TypeKind::Generic(_, e_args, _) => {
            if let TypeKind::Generic(_, a_args, _) = table.get(actual.0) {
                let e_ids = table.get_list(e_args).to_vec();
                let a_ids = table.get_list(a_args).to_vec();
                for (ea, aa) in e_ids.iter().zip(a_ids.iter()) {
                    collect_type_inferences(
                        &Type::resolved(*ea),
                        &Type::resolved(*aa),
                        params,
                        out,
                        table,
                        interner,
                    );
                }
            }
        }
        TypeKind::Array(e_inner) => {
            if let TypeKind::Array(a_inner) = table.get(actual.0) {
                collect_type_inferences(
                    &Type::resolved(e_inner),
                    &Type::resolved(a_inner),
                    params,
                    out,
                    table,
                    interner,
                );
            }
        }
        TypeKind::Fn(e_fid) => {
            if let TypeKind::Fn(a_fid) = table.get(actual.0) {
                let e_ft = table.get_function(e_fid).clone();
                let a_ft = table.get_function(a_fid).clone();
                let mut paired = e_ft.params.len();
                if let Some(pack) = e_ft.params.last().filter(|p| p.is_rest)
                    && let Some(name) = pack_param_name(pack.ty, table)
                    && params.iter().any(|p| p.as_ref() == interner.resolve(name))
                {
                    paired = e_ft.params.len() - 1;
                    let fixed = e_ft.params.len() - 1;
                    let slots: Vec<vn_sem::types::CheckerTyId> =
                        a_ft.params.iter().skip(fixed).map(|p| p.ty).collect();
                    let slot_list = table.intern_list(&slots);
                    let tuple = Type::resolved(table.intern(TypeKind::Tuple(slot_list)));
                    out.insert(Arc::from(interner.resolve(name)), tuple);
                }
                for (ep, ap) in e_ft.params.iter().take(paired).zip(a_ft.params.iter()) {
                    let mut contravariant = FxHashMap::default();
                    collect_type_inferences(
                        &Type::resolved(ep.ty),
                        &Type::resolved(ap.ty),
                        params,
                        &mut contravariant,
                        table,
                        interner,
                    );
                    for (name, ty) in contravariant {
                        out.entry(name).or_insert(ty);
                    }
                }
                collect_type_inferences(
                    &Type::resolved(e_ft.return_type),
                    &Type::resolved(a_ft.return_type),
                    params,
                    out,
                    table,
                    interner,
                );
            }
        }
        TypeKind::Union(e_members) => {
            let present =
                |ids: &[vn_sem::types::CheckerTyId]| -> Vec<vn_sem::types::CheckerTyId> {
                    ids.iter()
                        .copied()
                        .filter(|id| *id != Type::Null.0)
                        .collect()
                };
            let e_ids = present(table.get_list(e_members));
            let a_ids = if let TypeKind::Union(a_members) = table.get(actual.0) {
                present(table.get_list(a_members))
            } else {
                present(&[actual.0])
            };
            for (ea, aa) in e_ids.iter().zip(a_ids.iter()) {
                collect_type_inferences(
                    &Type::resolved(*ea),
                    &Type::resolved(*aa),
                    params,
                    out,
                    table,
                    interner,
                );
            }
        }
        TypeKind::Tuple(e_elems) => {
            if let TypeKind::Tuple(a_elems) = table.get(actual.0) {
                let e_ids = table.get_list(e_elems).to_vec();
                let a_ids = table.get_list(a_elems).to_vec();
                for (ea, aa) in e_ids.iter().zip(a_ids.iter()) {
                    collect_type_inferences(
                        &Type::resolved(*ea),
                        &Type::resolved(*aa),
                        params,
                        out,
                        table,
                        interner,
                    );
                }
            }
        }
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Intersection(_)
        | TypeKind::Named(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => {}
    }
}

fn pack_param_name(
    rest_ty: vn_sem::types::CheckerTyId,
    table: &CheckerTyTable,
) -> Option<vn_core::Atom> {
    let element = match table.get(rest_ty) {
        TypeKind::Array(inner) => inner,
        TypeKind::Named(..) => rest_ty,
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => return None,
    };
    match table.get(element) {
        TypeKind::Named(name, _) => Some(name),
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => None,
    }
}
