use super::compat_fn_sig::{fn_signature_compatible_type, types_compatible_with_fn_signature};
use super::core::types_compatible_impl;
use crate::bind::BindView;
use crate::types::{
    CheckerTyId, CheckerTyTable, ClassMemberInfo, ClassMemberKind, ObjectTypeMember, Type,
};
use rustc_hash::{FxHashMap, FxHashSet};

#[inline]
pub(super) fn t(id: CheckerTyId) -> Type {
    Type::resolved(id)
}

pub(super) fn is_known_named(bind: &BindView, name: &str) -> bool {
    bind.bind.has_named_type(name)
        || bind
            .bind
            .interner
            .get(name)
            .and_then(|atom| {
                bind.bind
                    .scopes
                    .get(bind.bind.global_scope)
                    .resolve(atom, &bind.bind.scopes)
            })
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
    table: &CheckerTyTable,
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
    use crate::types::TypeContext;

    if bind.get_class_members(declared, origin_decl).is_some() {
        if bind.get_class_members(inferred, origin_inf).is_none() {
            return false;
        }
        let mut current = inferred;
        loop {
            if current == declared {
                return true;
            }
            match bind.bind.get_class_parent(current) {
                Some(parent) => current = parent.name.as_ref(),
                None => return false,
            }
        }
    }

    let decl_members = named_members(bind, declared, origin_decl);
    let inf_members = named_members(bind, inferred, origin_inf);

    match (decl_members, inf_members) {
        (Some(decl), Some(inf)) => {
            class_members_compatible(&decl, &inf, bind, cache, in_progress, table)
        }
        _ => false,
    }
}

fn class_members_compatible(
    decl_members: &[ClassMemberInfo],
    inf_members: &[ClassMemberInfo],
    bind: &BindView,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
    for dm in decl_members {
        match dm.kind {
            ClassMemberKind::Property | ClassMemberKind::Getter | ClassMemberKind::Setter => {
                let found = inf_members
                    .iter()
                    .find(|im| im.name == dm.name)
                    .map(|m| m.ty);
                match found {
                    Some(inf_ty) => {
                        if !types_compatible_impl(
                            &dm.ty,
                            &inf_ty,
                            Some(bind),
                            cache,
                            in_progress,
                            table,
                        ) {
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
                if !types_compatible_impl(&dm.ty, &inf_m.ty, Some(bind), cache, in_progress, table)
                {
                    return false;
                }
            }
            ClassMemberKind::Constructor
            | ClassMemberKind::Function
            | ClassMemberKind::Variable
            | ClassMemberKind::Class
            | ClassMemberKind::Interface
            | ClassMemberKind::Namespace
            | ClassMemberKind::Enum
            | ClassMemberKind::Struct => {}
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
    table: &CheckerTyTable,
) -> bool {
    for dm in decl_members {
        match dm.kind {
            ClassMemberKind::Property | ClassMemberKind::Getter | ClassMemberKind::Setter => {
                let found = inf_fields.iter().find_map(|im| match im {
                    ObjectTypeMember::Property { name, ty, .. } if name == &dm.name => Some(*ty),
                    ObjectTypeMember::Property { .. }
                    | ObjectTypeMember::Method { .. }
                    | ObjectTypeMember::Index { .. }
                    | ObjectTypeMember::Callable { .. } => None,
                });
                match found {
                    Some(inf_ty) => {
                        if !types_compatible_impl(
                            &dm.ty,
                            &t(inf_ty),
                            Some(bind),
                            cache,
                            in_progress,
                            table,
                        ) {
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
                    } if name == &dm.name => Some((params.as_slice(), *return_type, *is_arrow)),
                    ObjectTypeMember::Property { .. }
                    | ObjectTypeMember::Method { .. }
                    | ObjectTypeMember::Index { .. }
                    | ObjectTypeMember::Callable { .. } => None,
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
                    table,
                ) {
                    return false;
                }
            }
            ClassMemberKind::Constructor
            | ClassMemberKind::Function
            | ClassMemberKind::Variable
            | ClassMemberKind::Class
            | ClassMemberKind::Interface
            | ClassMemberKind::Namespace
            | ClassMemberKind::Enum
            | ClassMemberKind::Struct => {}
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
    table: &CheckerTyTable,
) -> bool {
    for dm in decl_fields {
        match dm {
            ObjectTypeMember::Property {
                name, ty, optional, ..
            } => {
                let found = inf_members.iter().find(|im| &im.name == name).map(|m| m.ty);
                match found {
                    Some(inf_ty) => {
                        if !types_compatible_impl(
                            &t(*ty),
                            &inf_ty,
                            Some(bind),
                            cache,
                            in_progress,
                            table,
                        ) {
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
                    *return_type,
                    *is_arrow,
                    &inf_m.ty,
                    Some(bind),
                    cache,
                    in_progress,
                    table,
                ) {
                    return false;
                }
            }
            ObjectTypeMember::Index { .. } | ObjectTypeMember::Callable { .. } => {}
        }
    }
    true
}
