use super::types_compatible_impl;
use crate::binder::BindView;
use crate::types::{ClassMemberInfo, ClassMemberKind, ObjectTypeMember, Type};
use crate::types::{FunctionParam, FunctionType};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::TypeKind;

pub(super) fn is_known_named(bind: &BindView, name: &str) -> bool {
    bind.bind.has_named_type(name)
        || bind
            .bind
            .scopes
            .get(bind.bind.global_scope)
            .resolve(name, &bind.bind.scopes)
            .is_some()
}

pub(super) fn named_members(
    bind: &BindView,
    name: &str,
    origin: Option<&str>,
) -> Option<Vec<ClassMemberInfo>> {
    use crate::types::TypeContext;
    bind.get_interface_members(name, origin)
        .or_else(|| bind.get_class_members(name, origin))
        .or_else(|| bind.get_namespace_members(name, origin))
        .or_else(|| bind.get_enum_members(name, origin))
}

pub(super) fn compatible_named(
    declared: &str,
    origin_decl: Option<&str>,
    inferred: &str,
    origin_inf: Option<&str>,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
) -> bool {
    if declared == inferred {
        if origin_decl == origin_inf || origin_decl.is_none() || origin_inf.is_none() {
            return true;
        }
        let origin_decl_str = origin_decl.unwrap_or("");
        let origin_inf_str = origin_inf.unwrap_or("");
        if origin_decl_str == origin_inf_str
            || origin_decl_str.ends_with(origin_inf_str)
            || origin_inf_str.ends_with(origin_decl_str)
        {
            return true;
        }
    }
    let Some(bind) = bind else {
        return true;
    };

    let decl_members = named_members(bind, declared, origin_decl);
    let inf_members = named_members(bind, inferred, origin_inf);

    match (decl_members, inf_members) {
        (Some(decl), Some(inf)) => class_members_compatible(&decl, &inf, bind, cache, in_progress),
        _ => !is_known_named(bind, declared) || !is_known_named(bind, inferred),
    }
}

fn class_members_compatible(
    decl_members: &[ClassMemberInfo],
    inf_members: &[ClassMemberInfo],
    bind: &BindView,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
) -> bool {
    for dm in decl_members {
        match dm.kind {
            ClassMemberKind::Property | ClassMemberKind::Getter | ClassMemberKind::Setter => {
                let found = inf_members
                    .iter()
                    .find(|im| im.name == dm.name)
                    .map(|m| &m.ty);
                match found {
                    Some(inf_ty) => {
                        if !types_compatible_impl(&dm.ty, inf_ty, Some(bind), cache, in_progress) {
                            return false;
                        }
                    }
                    None if !dm.is_optional => return false,
                    None => {}
                }
            }
            ClassMemberKind::Method => {
                if dm.is_optional {
                    continue;
                }
                let Some(inf_m) = inf_members.iter().find(|im| im.name == dm.name) else {
                    return false;
                };
                if inf_m.kind != ClassMemberKind::Method {
                    return false;
                }
                if !types_compatible_impl(&dm.ty, &inf_m.ty, Some(bind), cache, in_progress) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

pub(super) fn class_members_match_object(
    decl_members: &[ClassMemberInfo],
    inf_fields: &[ObjectTypeMember],
    bind: &BindView,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
) -> bool {
    for dm in decl_members {
        match dm.kind {
            ClassMemberKind::Property | ClassMemberKind::Getter | ClassMemberKind::Setter => {
                let found = inf_fields.iter().find_map(|im| match im {
                    ObjectTypeMember::Property { name, ty, .. } if name == &dm.name => Some(ty),
                    _ => None,
                });
                match found {
                    Some(inf_ty) => {
                        if !types_compatible_impl(&dm.ty, inf_ty, Some(bind), cache, in_progress) {
                            return false;
                        }
                    }
                    None if !dm.is_optional => return false,
                    None => {}
                }
            }
            ClassMemberKind::Method => {
                if dm.is_optional {
                    continue;
                }
                let found = inf_fields.iter().find_map(|im| match im {
                    ObjectTypeMember::Method {
                        name,
                        params,
                        return_type,
                        is_arrow,
                        ..
                    } if name == &dm.name => {
                        Some((params.as_slice(), return_type.as_ref(), *is_arrow))
                    }
                    _ => None,
                });
                let Some((params, return_type, is_arrow)) = found else {
                    return false;
                };
                if !types_compatible_with_fn_signature(
                    &dm.ty,
                    params,
                    return_type,
                    is_arrow,
                    Some(bind),
                    cache,
                    in_progress,
                ) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

pub(super) fn object_matches_class_members(
    decl_fields: &[ObjectTypeMember],
    inf_members: &[ClassMemberInfo],
    bind: &BindView,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
) -> bool {
    for dm in decl_fields {
        match dm {
            ObjectTypeMember::Property {
                name, ty, optional, ..
            } => {
                let found = inf_members
                    .iter()
                    .find(|im| &im.name == name)
                    .map(|m| &m.ty);
                match found {
                    Some(inf_ty) => {
                        if !types_compatible_impl(ty, inf_ty, Some(bind), cache, in_progress) {
                            return false;
                        }
                    }
                    None if !*optional => return false,
                    None => {}
                }
            }
            ObjectTypeMember::Method {
                name,
                params,
                return_type,
                optional,
                is_arrow,
            } => {
                if *optional {
                    continue;
                }
                let Some(inf_m) = inf_members.iter().find(|im| &im.name == name) else {
                    return false;
                };
                if !fn_signature_compatible_type(
                    params,
                    return_type,
                    *is_arrow,
                    &inf_m.ty,
                    Some(bind),
                    cache,
                    in_progress,
                ) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

fn fn_signature_compatible_type(
    params: &[FunctionParam],
    return_type: &Type,
    is_arrow: bool,
    inferred: &Type,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
) -> bool {
    match &inferred.0 {
        TypeKind::Fn(ft2) => {
            let return_ok = matches!(return_type.0, TypeKind::Intrinsic(varn_core::TypeTag::Void))
                || types_compatible_impl(return_type, &ft2.return_type, bind, cache, in_progress);
            ft2.params.len() <= params.len()
                && return_ok
                && params.iter().zip(ft2.params.iter()).all(|(t1, t2)| {
                    types_compatible_impl(&t2.ty, &t1.ty, bind, cache, in_progress)
                        && t1.optional == t2.optional
                })
        }
        _ => {
            let declared = Type::fn_(FunctionType {
                params: params.to_vec(),
                return_type: Box::new(return_type.clone()),
                is_arrow,
                type_params: vec![],
            });
            types_compatible_impl(&declared, inferred, bind, cache, in_progress)
        }
    }
}

pub(super) fn types_compatible_with_fn_signature(
    declared: &Type,
    params: &[FunctionParam],
    return_type: &Type,
    is_arrow: bool,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
) -> bool {
    match &declared.0 {
        TypeKind::Fn(ft1) => {
            params.len() <= ft1.params.len()
                && types_compatible_impl(&ft1.return_type, return_type, bind, cache, in_progress)
                && ft1.params.iter().zip(params.iter()).all(|(t1, t2)| {
                    types_compatible_impl(&t2.ty, &t1.ty, bind, cache, in_progress)
                        && t1.optional == t2.optional
                })
        }
        _ => {
            let inferred = Type::fn_(FunctionType {
                params: params.to_vec(),
                return_type: Box::new(return_type.clone()),
                is_arrow,
                type_params: vec![],
            });
            types_compatible_impl(declared, &inferred, bind, cache, in_progress)
        }
    }
}
