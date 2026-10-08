use super::core::types_compatible_impl;
use super::resolve::is_intrinsic;
use super::scalar::t;
use crate::bind::BindView;
use crate::types::InternedTypeKind;
use crate::types::{CheckerTyTable, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::TypeKind;

pub(super) fn generic_arms(
    d: InternedTypeKind,
    i: InternedTypeKind,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> Option<bool> {
    match (d, i) {
        (TypeKind::Generic(n1, a1, _o1), TypeKind::Generic(n2, a2, _o2)) => {
            let l1 = table.get_list(a1);
            let l2 = table.get_list(a2);
            if is_intrinsic(bind, n1, varn_core::BuiltinType::Array.name())
                && is_intrinsic(bind, n2, varn_core::BuiltinType::Array.name())
                && l1.len() == 1
                && l2.len() == 1
            {
                Some(types_compatible_impl(
                    &t(l1[0]),
                    &t(l2[0]),
                    bind,
                    cache,
                    in_progress,
                    table,
                ))
            } else if n1 == n2 {
                Some(
                    l1.len() == l2.len()
                        && l1.to_vec().iter().zip(l2.to_vec().iter()).all(|(x, y)| {
                            types_compatible_impl(&t(*x), &t(*y), bind, cache, in_progress, table)
                        }),
                )
            } else {
                Some(false)
            }
        }
        _ => None,
    }
}
