mod helpers;

use crate::binder::BindResult;
use crate::types::{ObjectTypeMember, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::{IntrinsicType, MemberKey, TypeKind};

use self::helpers::{
    class_members_match_object, compatible_named, is_known_named, named_members,
    object_matches_class_members, template_matches_literal, types_compatible_with_fn_signature,
};

fn is_simple_type(ty: &Type) -> bool {
    matches!(
        &ty.0,
        TypeKind::Intrinsic(_)
            | TypeKind::LiteralInt(_)
            | TypeKind::LiteralFloat(_)
            | TypeKind::LiteralStr(_)
            | TypeKind::LiteralBool(_)
    )
}

fn simple_types_compatible(declared: &Type, inferred: &Type) -> bool {
    match (&declared.0, &inferred.0) {
        (TypeKind::Intrinsic(varn_core::TypeTag::Dynamic), _)
        | (_, TypeKind::Intrinsic(varn_core::TypeTag::Dynamic)) => true,

        (a, b) if a == b => true,

        (TypeKind::Intrinsic(varn_core::TypeTag::Int), TypeKind::LiteralInt(_)) => true,
        (TypeKind::Intrinsic(varn_core::TypeTag::Float), TypeKind::LiteralFloat(_)) => true,
        (
            TypeKind::Intrinsic(varn_core::TypeTag::Float),
            TypeKind::Intrinsic(varn_core::TypeTag::Int),
        ) => true,
        (
            TypeKind::Intrinsic(varn_core::TypeTag::Decimal),
            TypeKind::Intrinsic(varn_core::TypeTag::Int),
        ) => true,
        (
            TypeKind::Intrinsic(varn_core::TypeTag::Decimal),
            TypeKind::Intrinsic(varn_core::TypeTag::Float),
        ) => true,
        (
            TypeKind::Intrinsic(varn_core::TypeTag::BigInt),
            TypeKind::Intrinsic(varn_core::TypeTag::Int),
        ) => true,
        (TypeKind::Intrinsic(varn_core::TypeTag::Str), TypeKind::LiteralStr(_)) => true,
        (TypeKind::Intrinsic(varn_core::TypeTag::Bool), TypeKind::LiteralBool(_)) => true,
        (TypeKind::Intrinsic(varn_core::TypeTag::Char), TypeKind::LiteralStr(s))
            if s.chars().count() == 1 =>
        {
            true
        }
        _ => false,
    }
}

pub(crate) fn types_compatible(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindResult>,
) -> bool {
    let mut cache = FxHashMap::default();
    types_compatible_with_cache(declared, inferred, bind, &mut cache)
}

pub(crate) fn types_compatible_with_cache(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindResult>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
) -> bool {
    if declared.is_dynamic() || inferred.is_dynamic() {
        return true;
    }
    if declared == inferred {
        return true;
    }
    if is_simple_type(declared) && is_simple_type(inferred) {
        return simple_types_compatible(declared, inferred);
    }
    let mut in_progress = FxHashSet::default();
    types_compatible_impl(declared, inferred, bind, cache, &mut in_progress)
}

pub(super) fn types_compatible_impl(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindResult>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
) -> bool {
    if declared.is_dynamic() || inferred.is_dynamic() {
        return true;
    }
    if declared == inferred {
        return true;
    }
    if is_simple_type(declared) && is_simple_type(inferred) {
        return simple_types_compatible(declared, inferred);
    }
    let key = (
        declared.clone(),
        inferred.clone(),
        bind.map_or(0usize, |b| b as *const BindResult as usize),
    );
    if let Some(cached) = cache.get(&key) {
        return *cached;
    }
    if !in_progress.insert(key.clone()) {
        return true;
    }

    let result = match (&declared.0, &inferred.0) {
        (TypeKind::Intrinsic(varn_core::TypeTag::Dynamic), _)
        | (_, TypeKind::Intrinsic(varn_core::TypeTag::Dynamic)) => true,

        (a, b) if a == b => true,

        (TypeKind::Intrinsic(varn_core::TypeTag::Int), TypeKind::LiteralInt(_)) => true,
        (TypeKind::Intrinsic(varn_core::TypeTag::Float), TypeKind::LiteralFloat(_)) => true,
        (
            TypeKind::Intrinsic(varn_core::TypeTag::Float),
            TypeKind::Intrinsic(varn_core::TypeTag::Int),
        ) => true,
        (
            TypeKind::Intrinsic(varn_core::TypeTag::Decimal),
            TypeKind::Intrinsic(varn_core::TypeTag::Int),
        ) => true,
        (
            TypeKind::Intrinsic(varn_core::TypeTag::Decimal),
            TypeKind::Intrinsic(varn_core::TypeTag::Float),
        ) => true,
        (
            TypeKind::Intrinsic(varn_core::TypeTag::BigInt),
            TypeKind::Intrinsic(varn_core::TypeTag::Int),
        ) => true,
        (TypeKind::Intrinsic(varn_core::TypeTag::Str), TypeKind::LiteralStr(_)) => true,
        (TypeKind::Intrinsic(varn_core::TypeTag::Bool), TypeKind::LiteralBool(_)) => true,
        (TypeKind::Intrinsic(varn_core::TypeTag::Char), TypeKind::LiteralStr(s))
            if s.chars().count() == 1 =>
        {
            true
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::Str), TypeKind::TemplateLiteral(_)) => true,
        (TypeKind::TemplateLiteral(parts), TypeKind::LiteralStr(s)) => {
            template_matches_literal(parts, s)
        }
        (TypeKind::TemplateLiteral(a), TypeKind::TemplateLiteral(b)) => a == b,

        (TypeKind::Array(_), TypeKind::Array(inf_elem)) if inf_elem.is_dynamic() => true,

        (TypeKind::Array(decl_elem), TypeKind::Array(inf_elem)) => {
            types_compatible_impl(decl_elem, inf_elem, bind, cache, in_progress)
        }

        (TypeKind::Generic(name, args, _origin), TypeKind::Array(inner))
            if name.as_ref() == IntrinsicType::Array.as_str() && args.len() == 1 =>
        {
            if inner.is_dynamic() {
                return true;
            }
            types_compatible_impl(&args[0], inner, bind, cache, in_progress)
        }
        (TypeKind::Array(inner), TypeKind::Generic(name, args, _origin))
            if name.as_ref() == IntrinsicType::Array.as_str() && args.len() == 1 =>
        {
            types_compatible_impl(inner, &args[0], bind, cache, in_progress)
        }

        (TypeKind::Generic(n1, a1, _o1), TypeKind::Generic(n2, a2, _o2))
            if n1.as_ref() == IntrinsicType::Array.as_str()
                && n2.as_ref() == IntrinsicType::Array.as_str()
                && a1.len() == 1
                && a2.len() == 1 =>
        {
            types_compatible_impl(&a1[0], &a2[0], bind, cache, in_progress)
        }

        (TypeKind::Generic(n1, a1, _o1), TypeKind::Generic(n2, a2, _o2)) if n1 == n2 => {
            a1.len() == a2.len()
                && a1
                    .iter()
                    .zip(a2.iter())
                    .all(|(x, y)| types_compatible_impl(x, y, bind, cache, in_progress))
        }

        (TypeKind::Union(decl_members), TypeKind::Union(inf_members)) => {
            inf_members.iter().all(|im| {
                decl_members
                    .iter()
                    .any(|dm| types_compatible_impl(dm, im, bind, cache, in_progress))
            })
        }
        (TypeKind::Union(members), _) => members
            .iter()
            .any(|m| types_compatible_impl(m, inferred, bind, cache, in_progress)),
        (_, TypeKind::Union(inf_members)) => inf_members
            .iter()
            .all(|m| types_compatible_impl(declared, m, bind, cache, in_progress)),
        (_, TypeKind::Intrinsic(varn_core::TypeTag::Never)) => true,

        // Some intrinsics (`str`, `Error`, …) are also nameable declarations, so
        // the same type reaches here spelled two ways: an annotation resolves to
        // `Intrinsic(tag)` while `new Error(…)` infers `Named("Error")` from the
        // class symbol. One spelling, one type. Restricted to the bare `Named`
        // form on purpose — a `Generic` spelling carries type arguments the
        // intrinsic side has nothing to check against.
        (TypeKind::Intrinsic(tag), TypeKind::Named(name, _))
        | (TypeKind::Named(name, _), TypeKind::Intrinsic(tag))
            if IntrinsicType::from_str(name).is_some_and(|it| it.0 == *tag) =>
        {
            true
        }

        (TypeKind::Named(dn, origin_d), TypeKind::Named(in_, origin_i))
        | (TypeKind::Named(dn, origin_d), TypeKind::Generic(in_, _, origin_i))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Named(in_, origin_i))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Generic(in_, _, origin_i)) => {
            compatible_named(
                dn,
                origin_d.as_deref(),
                in_,
                origin_i.as_deref(),
                bind,
                cache,
                in_progress,
            )
        }
        (TypeKind::Named(dn, origin_d), TypeKind::Fn(ft))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Fn(ft)) => {
            let Some(bind) = bind else { return true };
            if let Some(members) = named_members(bind, dn, origin_d.as_deref()) {
                if let Some(callable) = members
                    .iter()
                    .find(|m| m.name.as_ref() == MemberKey::Callable.as_str())
                {
                    let fn_ty = Type(varn_core::TypeKind::Fn(ft.clone()), false);
                    return types_compatible_impl(
                        &callable.ty,
                        &fn_ty,
                        Some(bind),
                        cache,
                        in_progress,
                    );
                }
            }
            true
        }
        (TypeKind::Named(dn, origin_d), TypeKind::Object(inf_fields))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Object(inf_fields)) => {
            if let Some(bind) = bind {
                if let Some(decl_members) = named_members(bind, dn, origin_d.as_deref()) {
                    return class_members_match_object(
                        &decl_members,
                        inf_fields,
                        bind,
                        cache,
                        in_progress,
                    );
                }
                return !is_known_named(bind, dn);
            }
            true
        }
        (TypeKind::Object(decl_fields), TypeKind::Named(in_, origin_i))
        | (TypeKind::Object(decl_fields), TypeKind::Generic(in_, _, origin_i)) => {
            if let Some(bind) = bind {
                if let Some(inf_members) = named_members(bind, in_, origin_i.as_deref()) {
                    return object_matches_class_members(
                        decl_fields,
                        &inf_members,
                        bind,
                        cache,
                        in_progress,
                    );
                }
                return !is_known_named(bind, in_);
            }
            true
        }
        (TypeKind::Named(dn, _), _) => {
            if let Some(bind) = bind {
                !is_known_named(bind, dn)
            } else {
                true
            }
        }
        (_, TypeKind::Named(in_, _)) => {
            if let Some(bind) = bind {
                !is_known_named(bind, in_)
            } else {
                true
            }
        }
        (TypeKind::Generic(name, args, _origin), _)
            if name.as_ref() == IntrinsicType::Task.as_str() && args.len() == 1 =>
        {
            types_compatible_impl(&args[0], inferred, bind, cache, in_progress)
        }
        (TypeKind::Fn(ft1), TypeKind::Fn(ft2)) => {
            let return_ok = ft2.return_type.is_dynamic()
                || matches!(
                    ft1.return_type.0,
                    TypeKind::Intrinsic(varn_core::TypeTag::Void)
                )
                || types_compatible_impl(
                    &ft1.return_type,
                    &ft2.return_type,
                    bind,
                    cache,
                    in_progress,
                );
            ft2.params.len() <= ft1.params.len()
                && return_ok
                && ft1.params.iter().zip(ft2.params.iter()).all(|(t1, t2)| {
                    t2.ty.is_dynamic()
                        || matches!(&t2.ty.0, TypeKind::Named(_, _))
                        || (types_compatible_impl(&t2.ty, &t1.ty, bind, cache, in_progress)
                            && t1.optional == t2.optional)
                })
        }

        (TypeKind::Object(decl_fields), TypeKind::Object(inf_fields)) => {
            for dm in decl_fields {
                match dm {
                    ObjectTypeMember::Property {
                        name, ty, optional, ..
                    } => {
                        let found = inf_fields.iter().find_map(|im| match im {
                            ObjectTypeMember::Property {
                                name: iname,
                                ty: ity,
                                ..
                            } if iname == name => Some(ity),
                            _ => None,
                        });
                        match found {
                            Some(inf_ty) => {
                                if !types_compatible_impl(ty, inf_ty, bind, cache, in_progress) {
                                    return false;
                                }
                            }
                            None if !*optional => return false,
                            None => {}
                        }
                    }
                    ObjectTypeMember::Method {
                        name,
                        params: p1,
                        return_type: r1,
                        optional,
                        ..
                    } => {
                        if *optional {
                            continue;
                        }
                        let found = inf_fields.iter().find_map(|im| match im {
                            ObjectTypeMember::Method {
                                name: iname,
                                params: p2,
                                return_type: r2,
                                optional: o2,
                                ..
                            } if iname == name => Some((p2, r2, o2)),
                            _ => None,
                        });
                        match found {
                            Some((p2, r2, o2)) => {
                                if *optional != *o2
                                    || !types_compatible_impl(r1, r2, bind, cache, in_progress)
                                    || p1.len() != p2.len()
                                    || p1.iter().zip(p2.iter()).any(|(t1, t2)| {
                                        !types_compatible_impl(
                                            &t1.ty,
                                            &t2.ty,
                                            bind,
                                            cache,
                                            in_progress,
                                        ) || t1.optional != t2.optional
                                    })
                                {
                                    return false;
                                }
                            }
                            None => return false,
                        }
                    }
                    ObjectTypeMember::Index {
                        key_ty, value_ty, ..
                    } => {
                        let has_compatible_index = inf_fields.iter().any(|im| match im {
                            ObjectTypeMember::Index {
                                key_ty: ikey,
                                value_ty: ivalue,
                                ..
                            } => {
                                types_compatible_impl(key_ty, ikey, bind, cache, in_progress)
                                    && types_compatible_impl(
                                        value_ty,
                                        ivalue,
                                        bind,
                                        cache,
                                        in_progress,
                                    )
                            }
                            _ => false,
                        });
                        if has_compatible_index {
                            continue;
                        }

                        let explicit_members_compatible = inf_fields.iter().all(|im| match im {
                            ObjectTypeMember::Property { ty, .. } => {
                                types_compatible_impl(value_ty, ty, bind, cache, in_progress)
                            }
                            ObjectTypeMember::Method {
                                params,
                                return_type,
                                is_arrow,
                                ..
                            } => types_compatible_with_fn_signature(
                                value_ty,
                                params,
                                return_type,
                                *is_arrow,
                                bind,
                                cache,
                                in_progress,
                            ),
                            _ => true,
                        });
                        if !explicit_members_compatible {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            true
        }

        (TypeKind::Tuple(decl_elems), TypeKind::Array(inf_elem)) => decl_elems
            .iter()
            .all(|d| types_compatible_impl(d, inf_elem, bind, cache, in_progress)),

        (TypeKind::Tuple(decl_elems), TypeKind::Tuple(inf_elems)) => {
            decl_elems.len() == inf_elems.len()
                && decl_elems
                    .iter()
                    .zip(inf_elems)
                    .all(|(d, i)| types_compatible_impl(d, i, bind, cache, in_progress))
        }

        (TypeKind::Intersection(decl_members), _) => decl_members
            .iter()
            .all(|m| types_compatible_impl(m, inferred, bind, cache, in_progress)),

        (_, TypeKind::Intersection(inf_members)) => inf_members
            .iter()
            .any(|m| types_compatible_impl(declared, m, bind, cache, in_progress)),

        _ => false,
    };

    in_progress.remove(&key);
    cache.insert(key, result);
    result
}
