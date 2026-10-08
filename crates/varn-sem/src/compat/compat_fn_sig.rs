use super::compat_lookup::t;
use super::core::types_compatible_impl;
use crate::bind::BindView;
use crate::types::{CheckerTyId, CheckerTyTable, FunctionParam, FunctionType, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::TypeKind;

pub(super) fn fn_signature_compatible_type(
    params: &[FunctionParam],
    return_type: CheckerTyId,
    is_arrow: bool,
    inferred: &Type,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
    match table.get(inferred.0) {
        TypeKind::Fn(fid2) => {
            let ft2 = table.get_function(fid2).clone();
            let return_ok = matches!(
                table.get(return_type),
                TypeKind::Primitive(varn_core::LangPrimitive::Void)
            ) || types_compatible_impl(
                &t(return_type),
                &t(ft2.return_type),
                bind,
                cache,
                in_progress,
                table,
            );
            ft2.params.len() <= params.len()
                && return_ok
                && params.iter().zip(ft2.params.iter()).all(|(t1, t2)| {
                    types_compatible_impl(&t(t2.ty), &t(t1.ty), bind, cache, in_progress, table)
                        && t1.optional == t2.optional
                })
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => {
            let mut owned_table = table.clone();
            let declared = Type::fn_(
                FunctionType {
                    params: params.to_vec(),
                    return_type,
                    is_arrow,
                    type_params: vec![],
                },
                &mut owned_table,
            );
            types_compatible_impl(&declared, inferred, bind, cache, in_progress, &owned_table)
        }
    }
}
pub(super) fn types_compatible_with_fn_signature(
    declared: &Type,
    params: &[FunctionParam],
    return_type: CheckerTyId,
    is_arrow: bool,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
    match table.get(declared.0) {
        TypeKind::Fn(fid1) => {
            let ft1 = table.get_function(fid1).clone();
            params.len() <= ft1.params.len()
                && types_compatible_impl(
                    &t(ft1.return_type),
                    &t(return_type),
                    bind,
                    cache,
                    in_progress,
                    table,
                )
                && ft1.params.iter().zip(params.iter()).all(|(t1, t2)| {
                    types_compatible_impl(&t(t2.ty), &t(t1.ty), bind, cache, in_progress, table)
                        && t1.optional == t2.optional
                })
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => {
            let mut owned_table = table.clone();
            let inferred = Type::fn_(
                FunctionType {
                    params: params.to_vec(),
                    return_type,
                    is_arrow,
                    type_params: vec![],
                },
                &mut owned_table,
            );
            types_compatible_impl(declared, &inferred, bind, cache, in_progress, &owned_table)
        }
    }
}
