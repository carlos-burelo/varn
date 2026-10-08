use super::core::types_compatible_impl;
use super::resolve::is_intrinsic;
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
        (TypeKind::Array(decl_elem), TypeKind::Array(inf_elem)) => Some(types_compatible_impl(
            &t(decl_elem),
            &t(inf_elem),
            bind,
            cache,
            in_progress,
            table,
        )),
        (TypeKind::Generic(name, args, _origin), TypeKind::Array(inner)) => {
            let list = table.get_list(args);
            if is_intrinsic(bind, name, varn_core::BuiltinType::Array.name()) && list.len() == 1 {
                if t(inner).is_dynamic() {
                    Some(true)
                } else {
                    Some(types_compatible_impl(
                        &t(list[0]),
                        &t(inner),
                        bind,
                        cache,
                        in_progress,
                        table,
                    ))
                }
            } else {
                Some(false)
            }
        }
        (TypeKind::Array(inner), TypeKind::Generic(name, args, _origin)) => {
            let list = table.get_list(args);
            if is_intrinsic(bind, name, varn_core::BuiltinType::Array.name()) && list.len() == 1 {
                Some(types_compatible_impl(
                    &t(inner),
                    &t(list[0]),
                    bind,
                    cache,
                    in_progress,
                    table,
                ))
            } else {
                Some(false)
            }
        }
        _ => None,
    }
}
