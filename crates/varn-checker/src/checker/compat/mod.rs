mod helpers;

use crate::binder::{BindResult, BindView};
use crate::types::{ObjectTypeMember, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::{IntrinsicType, MemberKey, TypeKind};

use self::helpers::{
    class_members_match_object, compatible_named, is_known_named, named_members,
    object_matches_class_members, types_compatible_with_fn_signature,
};

fn is_simple_type(ty: &Type) -> bool {
    matches!(&ty.0, TypeKind::Intrinsic(_))
}

fn simple_types_compatible(declared: &Type, inferred: &Type) -> bool {
    match (&declared.0, &inferred.0) {
        (TypeKind::Intrinsic(varn_core::TypeTag::Dynamic), _)
        | (_, TypeKind::Intrinsic(varn_core::TypeTag::Dynamic)) => true,

        (a, b) if a == b => true,

        (TypeKind::Intrinsic(varn_core::TypeTag::Int), TypeKind::Intrinsic(inf_tag)) => {
            matches!(
                inf_tag,
                varn_core::TypeTag::I8
                    | varn_core::TypeTag::I16
                    | varn_core::TypeTag::I32
                    | varn_core::TypeTag::U8
                    | varn_core::TypeTag::U16
                    | varn_core::TypeTag::U32
            )
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::I32), TypeKind::Intrinsic(inf_tag)) => {
            matches!(
                inf_tag,
                varn_core::TypeTag::I8
                    | varn_core::TypeTag::I16
                    | varn_core::TypeTag::U8
                    | varn_core::TypeTag::U16
            )
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::I16), TypeKind::Intrinsic(inf_tag)) => {
            matches!(inf_tag, varn_core::TypeTag::I8 | varn_core::TypeTag::U8)
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::I8), _) => false,
        (TypeKind::Intrinsic(varn_core::TypeTag::U64), TypeKind::Intrinsic(inf_tag)) => {
            matches!(
                inf_tag,
                varn_core::TypeTag::U8
                    | varn_core::TypeTag::U16
                    | varn_core::TypeTag::U32
            )
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::U32), TypeKind::Intrinsic(inf_tag)) => {
            matches!(inf_tag, varn_core::TypeTag::U8 | varn_core::TypeTag::U16)
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::U16), TypeKind::Intrinsic(inf_tag)) => {
            matches!(inf_tag, varn_core::TypeTag::U8)
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::U8), _) => false,
        (TypeKind::Intrinsic(varn_core::TypeTag::Float), TypeKind::Intrinsic(inf_tag)) => {
            matches!(
                inf_tag,
                varn_core::TypeTag::Int
                    | varn_core::TypeTag::F32
                    | varn_core::TypeTag::I8
                    | varn_core::TypeTag::I16
                    | varn_core::TypeTag::I32
                    | varn_core::TypeTag::U8
                    | varn_core::TypeTag::U16
                    | varn_core::TypeTag::U32
            )
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::F32), TypeKind::Intrinsic(inf_tag)) => {
            matches!(
                inf_tag,
                varn_core::TypeTag::I8
                    | varn_core::TypeTag::I16
                    | varn_core::TypeTag::U8
                    | varn_core::TypeTag::U16
            )
        }
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
        _ => false,
    }
}

pub(crate) fn literal_fits_type(target: &Type, int_val: i64) -> bool {
    match &target.0 {
        TypeKind::Intrinsic(varn_core::TypeTag::I8) => (i8::MIN as i64..=i8::MAX as i64).contains(&int_val),
        TypeKind::Intrinsic(varn_core::TypeTag::I16) => (i16::MIN as i64..=i16::MAX as i64).contains(&int_val),
        TypeKind::Intrinsic(varn_core::TypeTag::I32) => (i32::MIN as i64..=i32::MAX as i64).contains(&int_val),
        TypeKind::Intrinsic(varn_core::TypeTag::U8) => (0..=u8::MAX as i64).contains(&int_val),
        TypeKind::Intrinsic(varn_core::TypeTag::U16) => (0..=u16::MAX as i64).contains(&int_val),
        TypeKind::Intrinsic(varn_core::TypeTag::U32) => (0..=u32::MAX as i64).contains(&int_val),
        TypeKind::Intrinsic(varn_core::TypeTag::U64) => int_val >= 0,
        TypeKind::Intrinsic(varn_core::TypeTag::Int) => true,
        TypeKind::Intrinsic(varn_core::TypeTag::Float | varn_core::TypeTag::F32) => true,
        TypeKind::Intrinsic(varn_core::TypeTag::Decimal | varn_core::TypeTag::BigInt | varn_core::TypeTag::Dynamic) => true,
        _ => false,
    }
}

/// Folds the sign/parens a narrow literal is written with, so `-128` reaches
/// the range check as `-128` and not as "some unary expression of type int".
/// The parser keeps `-128` as `Unary(Minus, IntLiteral(128))`; without this the
/// only narrow lower bounds expressible were the ones a `as i8` cast spelled out.
fn const_int_value(expr: &varn_core::ast::Expr) -> Option<i64> {
    use varn_core::ast::{ExprKind, UnaryOp};
    match &expr.kind {
        ExprKind::IntLiteral { value, .. } => Some(*value),
        ExprKind::Paren { expression } => const_int_value(expression),
        ExprKind::Unary {
            op, prefix: true, operand, ..
        } => match op {
            UnaryOp::Minus => const_int_value(operand).and_then(|v| v.checked_neg()),
            UnaryOp::Plus => const_int_value(operand),
            _ => None,
        },
        _ => None,
    }
}

fn is_numeric_literal(expr: &varn_core::ast::Expr) -> bool {
    use varn_core::ast::{ExprKind, UnaryOp};
    match &expr.kind {
        ExprKind::IntLiteral { .. } | ExprKind::FloatLiteral { .. } => true,
        ExprKind::Paren { expression } => is_numeric_literal(expression),
        ExprKind::Unary {
            op: UnaryOp::Minus | UnaryOp::Plus,
            prefix: true,
            operand,
            ..
        } => is_numeric_literal(operand),
        _ => false,
    }
}

fn array_element_type(ty: &Type) -> Option<&Type> {
    match &ty.0 {
        TypeKind::Array(inner) => Some(inner),
        TypeKind::Generic(name, args, _)
            if name.as_ref() == IntrinsicType::Array.as_str() && args.len() == 1 =>
        {
            Some(&args[0])
        }
        _ => None,
    }
}

/// Assignability escape hatch for *literals* written at a narrow target type.
///
/// `types_compatible` is a pure type-to-type relation and a narrow type is
/// deliberately not a supertype of `int` (`simple_types_compatible` answers
/// `(I8, _) => false`), so `let x: i8 = 42` can only be accepted by looking at
/// the literal's value rather than at its inferred type. That is what this
/// function is for, and `value_assignable_to` is the single funnel that pairs
/// the two.
///
/// Array literals need the same treatment one level down. `[1, 2, 3]` infers as
/// `int[]`, and `Array<i8>` vs `int[]` bottoms out in the very same
/// `simple_types_compatible(I8, Int) == false`, so before this the *valid*
/// program `let a: Array<i8> = [1,2,3]` was a type error and there was no path
/// on which an out-of-range element could ever be range-checked. Recursing into
/// the literal's elements here — and pointing `check_array_with_context` at
/// `value_assignable_to` instead of raw `types_compatible` — is what makes
/// `[1,2,3]` legal and `[300]` a compile error rather than a later silent
/// truncation into a compact `ArrayRepr`.
pub(crate) fn expr_satisfies_target_type(
    target_ty: &Type,
    _init_ty: &Type,
    expr: Option<&varn_core::ast::Expr>,
) -> bool {
    let Some(expr) = expr else {
        return false;
    };
    use varn_core::ast::{ArrayEl, ExprKind};
    if let ExprKind::Paren { expression } = &expr.kind {
        return expr_satisfies_target_type(target_ty, _init_ty, Some(expression));
    }
    if target_ty.is_granular_int() {
        if let Some(value) = const_int_value(expr) {
            return literal_fits_type(target_ty, value);
        }
    }
    if matches!(target_ty.0, TypeKind::Intrinsic(varn_core::TypeTag::F32)) && is_numeric_literal(expr)
    {
        return true;
    }
    if let (Some(elem_ty), ExprKind::Array { elements }) = (array_element_type(target_ty), &expr.kind)
    {
        let narrow_elem = elem_ty.is_granular_int()
            || matches!(elem_ty.0, TypeKind::Intrinsic(varn_core::TypeTag::F32));
        if narrow_elem && !elements.is_empty() {
            return elements.iter().all(|el| match el {
                ArrayEl::Expr(e) => expr_satisfies_target_type(elem_ty, elem_ty, Some(e)),
                _ => false,
            });
        }
    }
    false
}

pub(crate) fn types_compatible(declared: &Type, inferred: &Type, bind: Option<&BindView>) -> bool {
    let mut cache = FxHashMap::default();
    types_compatible_with_cache(declared, inferred, bind, &mut cache)
}

pub(crate) fn types_compatible_with_cache(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindView>,
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
    bind: Option<&BindView>,
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
        bind.map_or(0usize, |b| b.bind as *const BindResult as usize),
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

        (TypeKind::Intrinsic(_), TypeKind::Intrinsic(_)) => {
            simple_types_compatible(declared, inferred)
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::Str), TypeKind::TemplateLiteral(_)) => true,
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
        (TypeKind::Generic(dn, args, _), TypeKind::Object(inf_fields))
            if dn.as_ref() == IntrinsicType::Map.as_str()
                && (args.len() == 1 || args.len() == 2) =>
        {
            let (key_ty, val_ty) = if args.len() == 2 {
                (&args[0], &args[1])
            } else {
                (&Type::Str, &args[0])
            };
            let key_compat = types_compatible_impl(key_ty, &Type::Str, bind, cache, in_progress);
            if !key_compat {
                return false;
            }
            inf_fields.iter().all(|im| match im {
                ObjectTypeMember::Property { ty, .. } => {
                    types_compatible_impl(val_ty, ty, bind, cache, in_progress)
                }
                ObjectTypeMember::Index {
                    key_ty: ik,
                    value_ty: iv,
                    ..
                } => {
                    types_compatible_impl(key_ty, ik, bind, cache, in_progress)
                        && types_compatible_impl(val_ty, iv, bind, cache, in_progress)
                }
                _ => false,
            })
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::Map), TypeKind::Object(_))
        | (TypeKind::Object(_), TypeKind::Intrinsic(varn_core::TypeTag::Map)) => true,
        (TypeKind::Named(dn, origin_d), TypeKind::Object(inf_fields))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Object(inf_fields)) => {
            if dn.as_ref() == IntrinsicType::Map.as_str() {
                return true;
            }
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
        (TypeKind::Object(decl_fields), TypeKind::Generic(in_, args, _))
            if in_.as_ref() == IntrinsicType::Map.as_str()
                && (args.len() == 1 || args.len() == 2) =>
        {
            let (key_ty, val_ty) = if args.len() == 2 {
                (&args[0], &args[1])
            } else {
                (&Type::Str, &args[0])
            };
            decl_fields.iter().all(|dm| match dm {
                ObjectTypeMember::Index {
                    key_ty: dk,
                    value_ty: dv,
                    ..
                } => {
                    types_compatible_impl(dk, key_ty, bind, cache, in_progress)
                        && types_compatible_impl(dv, val_ty, bind, cache, in_progress)
                }
                ObjectTypeMember::Property { optional: true, .. } => true,
                _ => false,
            })
        }
        (TypeKind::Object(decl_fields), TypeKind::Named(in_, origin_i))
        | (TypeKind::Object(decl_fields), TypeKind::Generic(in_, _, origin_i)) => {
            if in_.as_ref() == IntrinsicType::Map.as_str() {
                return true;
            }
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
        (TypeKind::Named(dn, _), _) if dn.as_ref() == IntrinsicType::Map.as_str() => true,
        (_, TypeKind::Named(in_, _)) if in_.as_ref() == IntrinsicType::Map.as_str() => true,
        (TypeKind::Named(dn, origin_d), _) => {
            use crate::types::TypeContext;
            if let Some(bind) = bind {
                if let Some(expanded) = bind.resolve_type_alias(dn, origin_d.as_deref()) {
                    if &expanded != declared {
                        return types_compatible_impl(&expanded, inferred, Some(bind), cache, in_progress);
                    }
                }
            }
            false
        }
        (_, TypeKind::Named(in_, origin_i)) => {
            use crate::types::TypeContext;
            if let Some(bind) = bind {
                if let Some(expanded) = bind.resolve_type_alias(in_, origin_i.as_deref()) {
                    if &expanded != inferred {
                        return types_compatible_impl(declared, &expanded, Some(bind), cache, in_progress);
                    }
                }
            }
            false
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

            let has_index_decl = decl_fields
                .iter()
                .any(|m| matches!(m, ObjectTypeMember::Index { .. }));
            if !has_index_decl {
                for im in inf_fields {
                    if let ObjectTypeMember::Property { name: iname, .. } = im {
                        let exists_in_decl = decl_fields.iter().any(|dm| match dm {
                            ObjectTypeMember::Property { name: dname, .. } => dname == iname,
                            ObjectTypeMember::Method { name: dname, .. } => dname == iname,
                            _ => false,
                        });
                        if !exists_in_decl {
                            return false;
                        }
                    }
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
