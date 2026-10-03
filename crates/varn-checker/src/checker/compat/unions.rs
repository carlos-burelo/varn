use super::core::types_compatible_impl;
use super::scalar::t;
use crate::binder::BindView;
use crate::types::InternedTypeKind;
use crate::types::{CheckerTyTable, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::TypeKind;

pub(super) fn union_arms(
    declared: &Type,
    inferred: &Type,
    d: InternedTypeKind,
    i: InternedTypeKind,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> Option<bool> {
    match (d, i) {
        (TypeKind::Union(decl_members), TypeKind::Union(inf_members)) => {
            let decl_ids = table.get_list(decl_members).to_vec();
            let inf_ids = table.get_list(inf_members).to_vec();
            Some(inf_ids.iter().all(|im| {
                decl_ids.iter().any(|dm| {
                    types_compatible_impl(&t(*dm), &t(*im), bind, cache, in_progress, table)
                })
            }))
        }
        (TypeKind::Union(members), _) => {
            Some(
                table.get_list(members).to_vec().iter().any(|m| {
                    types_compatible_impl(&t(*m), inferred, bind, cache, in_progress, table)
                }),
            )
        }
        (_, TypeKind::Union(inf_members)) => {
            Some(
                table.get_list(inf_members).to_vec().iter().all(|m| {
                    types_compatible_impl(declared, &t(*m), bind, cache, in_progress, table)
                }),
            )
        }
        _ => None,
    }
}
