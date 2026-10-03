use crate::types::{CheckerTyTable, Type};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::TypeKind;

pub(crate) fn collect_type_inferences(
    expected: &Type,
    actual: &Type,
    params: &[Arc<str>],
    out: &mut FxHashMap<Arc<str>, Type>,
    table: &mut CheckerTyTable,
    interner: &varn_core::AtomInterner,
) {
    let expected_kind = table.get(expected.0);
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
                for (ep, ap) in e_ft.params.iter().zip(a_ft.params.iter()) {
                    collect_type_inferences(
                        &Type::resolved(ep.ty),
                        &Type::resolved(ap.ty),
                        params,
                        out,
                        table,
                        interner,
                    );
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
            if let TypeKind::Union(a_members) = table.get(actual.0) {
                let e_ids = table.get_list(e_members).to_vec();
                let a_ids = table.get_list(a_members).to_vec();
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
        _ => {}
    }
}
