use super::compat_lookup::{
    class_members_match_object, compatible_named, is_known_named, named_members,
    object_matches_class_members,
};
use super::core::types_compatible_impl;
use super::resolve::{is_intrinsic, m_ty, resolve_atom};
use super::scalar::t;
use crate::bind::BindView;
use crate::types::{CheckerTyTable, InternedTypeKind, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::{MemberKey, TypeKind};

pub(super) fn nominal_arms(
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
        (TypeKind::Named(dn, origin_d), TypeKind::Named(in_, origin_i))
        | (TypeKind::Named(dn, origin_d), TypeKind::Generic(in_, _, origin_i))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Named(in_, origin_i)) => {
            Some(match (resolve_atom(bind, dn), resolve_atom(bind, in_)) {
                (Some(dn_s), Some(in_s)) => compatible_named(
                    &dn_s,
                    origin_d.and_then(|o| resolve_atom(bind, o)).as_deref(),
                    &in_s,
                    origin_i.and_then(|o| resolve_atom(bind, o)).as_deref(),
                    bind,
                    cache,
                    in_progress,
                    table,
                ),
                _ => dn == in_,
            })
        }
        (TypeKind::Named(dn, origin_d), TypeKind::Fn(ft))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Fn(ft)) => {
            let (Some(bind), Some(dn_s)) = (bind, resolve_atom(bind, dn)) else {
                return Some(true);
            };
            let origin_d_s = origin_d.and_then(|o| resolve_atom(Some(bind), o));
            let _ = ft;
            if let Some(members) = named_members(bind, &dn_s, origin_d_s.as_deref()) {
                if let Some(callable) = members
                    .iter()
                    .find(|m| m.name.as_ref() == MemberKey::Callable.as_str())
                {
                    return Some(types_compatible_impl(
                        &m_ty(callable),
                        inferred,
                        Some(bind),
                        cache,
                        in_progress,
                        table,
                    ));
                }
            }
            Some(true)
        }
        (TypeKind::Generic(..), TypeKind::Object(_)) => Some(false),
        (TypeKind::Named(dn, origin_d), TypeKind::Object(inf_fields)) => Some(
            if let (Some(bind), Some(dn_s)) = (bind, resolve_atom(bind, dn)) {
                let origin_d_s = origin_d.and_then(|o| resolve_atom(Some(bind), o));
                if let Some(decl_members) = named_members(bind, &dn_s, origin_d_s.as_deref()) {
                    class_members_match_object(
                        &decl_members,
                        table.get_object_members(inf_fields),
                        bind,
                        cache,
                        in_progress,
                        table,
                    )
                } else {
                    !is_known_named(bind, &dn_s)
                }
            } else {
                true
            },
        ),
        (TypeKind::Object(_), TypeKind::Generic(..)) => Some(false),
        (TypeKind::Object(decl_fields), TypeKind::Named(in_, origin_i)) => Some(
            if let (Some(bind), Some(in_s)) = (bind, resolve_atom(bind, in_)) {
                let origin_i_s = origin_i.and_then(|o| resolve_atom(Some(bind), o));
                if let Some(inf_members) = named_members(bind, &in_s, origin_i_s.as_deref()) {
                    object_matches_class_members(
                        table.get_object_members(decl_fields),
                        &inf_members,
                        bind,
                        cache,
                        in_progress,
                        table,
                    )
                } else {
                    !is_known_named(bind, &in_s)
                }
            } else {
                true
            },
        ),
        (TypeKind::Named(dn, dn_origin), _) => Some(named_fallback(
            declared,
            inferred,
            dn,
            dn_origin,
            bind,
            cache,
            in_progress,
            table,
            true,
        )),
        (_, TypeKind::Named(in_, in_origin)) => Some(named_fallback(
            declared,
            inferred,
            in_,
            in_origin,
            bind,
            cache,
            in_progress,
            table,
            false,
        )),
        (TypeKind::Generic(name, args, _origin), _) => {
            let list = table.get_list(args);
            if is_intrinsic(bind, name, varn_core::BuiltinType::Task.name()) && list.len() == 1 {
                Some(types_compatible_impl(
                    &t(list[0]),
                    inferred,
                    bind,
                    cache,
                    in_progress,
                    table,
                ))
            } else {
                Some(false)
            }
        }
        (TypeKind::Fn(fid1), TypeKind::Fn(fid2)) => {
            let ft1 = table.get_function(fid1).clone();
            let ft2 = table.get_function(fid2).clone();
            let return_ok = t(ft2.return_type).is_dynamic()
                || matches!(
                    table.get(ft1.return_type),
                    TypeKind::Primitive(varn_core::LangPrimitive::Void)
                )
                || types_compatible_impl(
                    &t(ft1.return_type),
                    &t(ft2.return_type),
                    bind,
                    cache,
                    in_progress,
                    table,
                );
            Some(
                ft2.params.len() <= ft1.params.len()
                    && return_ok
                    && ft1.params.iter().zip(ft2.params.iter()).all(|(t1, t2)| {
                        t(t2.ty).is_dynamic()
                            || matches!(table.get(t2.ty), TypeKind::Named(_, _))
                            || (types_compatible_impl(
                                &t(t2.ty),
                                &t(t1.ty),
                                bind,
                                cache,
                                in_progress,
                                table,
                            ) && t1.optional == t2.optional)
                    }),
            )
        }
        _ => None,
    }
}
fn named_fallback(
    declared: &Type,
    inferred: &Type,
    name: varn_core::Atom,
    origin: Option<varn_core::Atom>,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
    is_declared: bool,
) -> bool {
    use crate::types::TypeContext;
    let Some(bind) = bind else { return false };
    let Some(name_s) = resolve_atom(Some(bind), name) else {
        return false;
    };
    let origin_s = origin.and_then(|o| resolve_atom(Some(bind), o));
    let Some(expanded) = bind.resolve_type_alias(&name_s, origin_s.as_deref()) else {
        return false;
    };
    if is_declared {
        if expanded.0 != declared.0 {
            return types_compatible_impl(
                &expanded,
                inferred,
                Some(bind),
                cache,
                in_progress,
                table,
            );
        }
    } else if expanded.0 != inferred.0 {
        return types_compatible_impl(declared, &expanded, Some(bind), cache, in_progress, table);
    }
    false
}
