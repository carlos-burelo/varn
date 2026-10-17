use super::core::types_compatible_impl;
use super::scalar::t;
use crate::bind::BindView;
use crate::types::InternedTypeKind;
use crate::types::{CheckerTyTable, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::TypeKind;

pub(super) fn array_arms(
    d: InternedTypeKind,
    i: InternedTypeKind,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> Option<bool> {
    match (d, i) {
        (TypeKind::Array(_), TypeKind::Array(inf_elem)) if t(inf_elem).is_dynamic() => Some(true),
        (TypeKind::Array(decl_elem), TypeKind::Array(inf_elem)) => Some(same_element(
            &t(decl_elem),
            &t(inf_elem),
            bind,
            cache,
            in_progress,
            table,
        )),
        (TypeKind::Generic(..), TypeKind::Array(_))
        | (TypeKind::Array(_), TypeKind::Generic(..)) => Some(false),
        _ => None,
    }
}

pub(super) fn same_element(
    declared: &Type,
    given: &Type,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
    if declared.is_dynamic() || given.is_dynamic() {
        return true;
    }
    types_compatible_impl(declared, given, bind, cache, in_progress, table)
        && types_compatible_impl(given, declared, bind, cache, in_progress, table)
}
