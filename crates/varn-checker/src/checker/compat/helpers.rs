use super::types_compatible_impl;
use crate::binder::BindResult;
use crate::types::{ClassMemberInfo, ClassMemberKind, ObjectTypeMember, Type};
use crate::types::{FunctionParam, FunctionType};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::TypeKind;

pub(super) fn is_known_named(bind: &BindResult, name: &str) -> bool {
    bind.has_named_type(name)
        || bind
            .scopes
            .get(bind.global_scope)
            .resolve(name, &bind.scopes)
            .is_some()
}

pub(super) fn named_members<'a>(bind: &'a BindResult, name: &str) -> Option<&'a [ClassMemberInfo]> {
    bind.get_interface_members_local(name)
        .or_else(|| bind.get_class_entry(name).map(|e| &e.members))
        .or_else(|| bind.get_namespace_members_local(name))
        .map(|v| v.as_slice())
}

pub(super) fn compatible_named(
    declared: &str,
    inferred: &str,
    bind: Option<&BindResult>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
) -> bool {
    if declared == inferred {
        return true;
    }
    let Some(bind) = bind else {
        return true;
    };

    let decl_members = named_members(bind, declared);
    let inf_members = named_members(bind, inferred);

    match (decl_members, inf_members) {
        (Some(decl), Some(inf)) => class_members_compatible(decl, inf, bind, cache, in_progress),
        _ => !is_known_named(bind, declared) || !is_known_named(bind, inferred),
    }
}

fn class_members_compatible(
    decl_members: &[ClassMemberInfo],
    inf_members: &[ClassMemberInfo],
    bind: &BindResult,
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
    bind: &BindResult,
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
    bind: &BindResult,
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

fn interp_accepts(ty: &Type, segment: &str) -> bool {
    use varn_core::TypeTag;
    match &ty.0 {
        TypeKind::Intrinsic(TypeTag::Str) => true,
        TypeKind::Intrinsic(TypeTag::Char) => segment.chars().count() == 1,
        TypeKind::Intrinsic(TypeTag::Int) => segment.parse::<i64>().is_ok(),
        TypeKind::Intrinsic(TypeTag::Float) | TypeKind::Intrinsic(TypeTag::Decimal) => {
            segment.parse::<f64>().is_ok()
        }
        TypeKind::Intrinsic(TypeTag::Bool) => segment == "true" || segment == "false",
        TypeKind::LiteralStr(s) => segment == s.as_ref(),
        TypeKind::LiteralInt(i) => segment == i.to_string(),
        TypeKind::LiteralBool(b) => segment == b.to_string(),
        TypeKind::TemplateLiteral(parts) => template_matches_literal(parts, segment),
        TypeKind::Union(members) => members.iter().any(|m| interp_accepts(m, segment)),
        TypeKind::Intrinsic(TypeTag::Dynamic) => true,
        _ => false,
    }
}

fn fn_signature_compatible_type(
    params: &[FunctionParam],
    return_type: &Type,
    is_arrow: bool,
    inferred: &Type,
    bind: Option<&BindResult>,
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
    bind: Option<&BindResult>,
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

pub(super) fn template_matches_literal(parts: &[Type], value: &str) -> bool {
    fn rec(parts: &[Type], idx: usize, remaining: &str) -> bool {
        if idx >= parts.len() {
            return remaining.is_empty();
        }

        if idx % 2 == 0 {
            let TypeKind::LiteralStr(prefix) = &parts[idx].0 else {
                return false;
            };
            if let Some(rest) = remaining.strip_prefix(prefix.as_ref()) {
                return rec(parts, idx + 1, rest);
            }
            return false;
        }

        let next_literal = if idx + 1 < parts.len() {
            if let TypeKind::LiteralStr(s) = &parts[idx + 1].0 {
                Some(s.as_ref())
            } else {
                None
            }
        } else {
            None
        };

        match next_literal {
            Some(lit) if !lit.is_empty() => {
                let mut starts = vec![];
                if remaining.starts_with(lit) {
                    starts.push(0);
                }
                starts.extend(
                    remaining
                        .match_indices(lit)
                        .map(|(i, _)| i)
                        .filter(|i| *i > 0),
                );
                starts.into_iter().any(|i| {
                    interp_accepts(&parts[idx], &remaining[..i])
                        && rec(parts, idx + 1, &remaining[i..])
                })
            }
            _ => interp_accepts(&parts[idx], remaining) && rec(parts, idx + 1, ""),
        }
    }

    rec(parts, 0, value)
}
