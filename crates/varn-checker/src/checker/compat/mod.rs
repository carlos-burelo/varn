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

fn const_float_value(expr: &varn_core::ast::Expr) -> Option<f64> {
    use varn_core::ast::{ExprKind, UnaryOp};
    match &expr.kind {
        ExprKind::FloatLiteral { value, .. } => Some(*value),
        ExprKind::IntLiteral { value, .. } => Some(*value as f64),
        ExprKind::Paren { expression } => const_float_value(expression),
        ExprKind::Unary {
            op, prefix: true, operand, ..
        } => match op {
            UnaryOp::Minus => const_float_value(operand).map(|v| -v),
            UnaryOp::Plus => const_float_value(operand),
            _ => None,
        },
        _ => None,
    }
}

/// The `f32` bound cannot live in [`literal_fits_type`]: that function takes an
/// `i64`, and every `i64` is inside `f32`'s exponent range, so it has no way to
/// express the case that actually loses data. A float literal is parsed as
/// `f64`, whose exponent range is far wider than `f32`'s — `1e300` narrows to
/// `inf`. Rejecting that is the float analogue of `300` not fitting an `i8`, and
/// it has to be rejected here rather than later: a compact `f32` array
/// representation would turn the overflow into a silent `inf`.
fn float_literal_fits_f32(value: f64) -> bool {
    !value.is_finite() || (value as f32).is_finite()
}

/// A *non*-narrow literal sitting in a recursive position — an object property
/// next to the narrow one — still has to be answered, and `types_compatible` is
/// not reachable from this pure function (it needs a `BindView`). Only literal
/// kinds whose type is unambiguous are handled, and for those the answer is
/// exactly what `types_compatible` would give, so this widens nothing: the
/// function is only ever consulted after `types_compatible` already said no
/// about the *whole* type.
fn plain_literal_matches(target: &Type, expr: &varn_core::ast::Expr) -> bool {
    use varn_core::ast::ExprKind;
    use varn_core::TypeTag;
    let TypeKind::Intrinsic(tag) = &target.0 else {
        return false;
    };
    matches!(
        (&expr.kind, tag),
        (ExprKind::StrLiteral { .. }, TypeTag::Str)
            | (ExprKind::BoolLiteral { .. }, TypeTag::Bool)
            | (ExprKind::CharLiteral { .. }, TypeTag::Char)
            | (ExprKind::IntLiteral { .. }, TypeTag::Int)
            | (ExprKind::FloatLiteral { .. }, TypeTag::Float)
    )
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
    if matches!(target_ty.0, TypeKind::Intrinsic(varn_core::TypeTag::F32)) {
        if let Some(value) = const_float_value(expr) {
            return float_literal_fits_f32(value);
        }
    }
    if let (Some(elem_ty), ExprKind::Array { elements }) = (array_element_type(target_ty), &expr.kind)
    {
        // `Array<Array<i8>>` recurses: the gate asks whether the element type is
        // narrow *or another array*, so nesting does not bail out one level in.
        let narrow_elem = elem_ty.is_granular_int()
            || matches!(elem_ty.0, TypeKind::Intrinsic(varn_core::TypeTag::F32))
            || array_element_type(elem_ty).is_some();
        if narrow_elem && !elements.is_empty() {
            return elements.iter().all(|el| match el {
                ArrayEl::Expr(e) => expr_satisfies_target_type(elem_ty, elem_ty, Some(e)),
                _ => false,
            });
        }
    }
    // An inline object type carries its members inline, so `{ xs: [1, 2] }`
    // against `{ xs: Array<i8> }` can recurse without a binder. A `Named`
    // interface cannot — resolving its members needs a `BindView` this pure
    // function has no access to — so that spelling still falls through.
    if let (TypeKind::Object(members), ExprKind::Object { properties }) =
        (&target_ty.0, &expr.kind)
    {
        if properties.is_empty() {
            return false;
        }
        let mut present: Vec<&str> = Vec::with_capacity(properties.len());
        for prop in properties {
            let varn_core::ast::ObjectProp::Property { key, value, .. } = prop else {
                return false;
            };
            let key_str = match key {
                varn_core::ast::PropKey::Identifier(s) | varn_core::ast::PropKey::Str(s) => {
                    s.as_str()
                }
                _ => return false,
            };
            let matched = members.iter().any(|m| match m {
                ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == key_str => {
                    expr_satisfies_target_type(ty, ty, Some(value))
                        || plain_literal_matches(ty, value)
                }
                _ => false,
            });
            if !matched {
                return false;
            }
            present.push(key_str);
        }
        // Checking only the literal's own properties is not enough: this is an
        // assignability answer, so a required member the literal omits has to
        // reject too, or `{ xs: Array<i8>, name: str } = { xs: [1, 2] }` would
        // leave a `str`-typed field holding null.
        let required_missing = members.iter().any(|m| match m {
            ObjectTypeMember::Property {
                name,
                optional: false,
                ..
            }
            | ObjectTypeMember::Method {
                name,
                optional: false,
                ..
            } => !present.contains(&name.as_ref()),
            _ => false,
        });
        return !required_missing;
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

#[cfg(test)]
mod tests {
    use super::*;
    use varn_core::TypeTag;

    /// Parses `let probe = <src>` and hands back the initializer AST.
    /// `expr_satisfies_target_type` is a pure function of (target type, expr),
    /// so a real parse is all the fixture the value-level checks need — no
    /// binder, no scopes.
    fn init_of(src: &str) -> varn_core::ast::Expr {
        let text = format!("let probe = {src}\n");
        let (tokens, lexemes, _) = varn_lexer::scan(&text, "compat-test");
        let program = varn_parser::parse(tokens, lexemes, "compat-test").expect("parses");
        for stmt in &program.body {
            if let varn_core::ast::StmtKind::Decl(decl) = &stmt.kind {
                if let varn_core::ast::Decl::Variable(v) = &**decl {
                    if let Some(init) = v.declarators[0].init.clone() {
                        return init;
                    }
                }
            }
        }
        panic!("no initializer parsed from `{src}`");
    }

    fn accepts(target: &Type, src: &str) -> bool {
        expr_satisfies_target_type(target, &Type::Dynamic, Some(&init_of(src)))
    }

    fn array_of(tag: TypeTag) -> Type {
        Type::array(Type::intrinsic(tag))
    }

    /// The safety property a later compact `ArrayRepr` depends on: an element
    /// that does not fit the declared narrow width must never be accepted, or it
    /// would be truncated silently at runtime instead.
    #[test]
    fn narrow_array_literal_rejects_out_of_range_element() {
        assert!(!accepts(&array_of(TypeTag::I8), "[300]"));
        assert!(!accepts(&array_of(TypeTag::I8), "[0, 1, -129]"));
        assert!(!accepts(&array_of(TypeTag::U8), "[-1]"));
        assert!(!accepts(&array_of(TypeTag::U32), "[4294967296]"));
    }

    #[test]
    fn narrow_array_literal_accepts_in_range_elements() {
        assert!(accepts(&array_of(TypeTag::I8), "[-128, 0, 127]"));
        assert!(accepts(&array_of(TypeTag::U8), "[0, 255]"));
        assert!(accepts(&array_of(TypeTag::U32), "[0, 4000000000]"));
    }

    #[test]
    fn f32_literal_that_would_narrow_to_infinity_is_rejected() {
        assert!(!accepts(&array_of(TypeTag::F32), "[1e300]"));
        assert!(!accepts(&Type::intrinsic(TypeTag::F32), "1e300"));
        assert!(accepts(&array_of(TypeTag::F32), "[1.5, -2.25, 3]"));
        assert!(accepts(&Type::intrinsic(TypeTag::F32), "3.14"));
    }

    #[test]
    fn nested_narrow_array_literals_recurse() {
        let nested = Type::array(array_of(TypeTag::I8));
        assert!(accepts(&nested, "[[1, 2], [3]]"));
        assert!(!accepts(&nested, "[[1, 2], [300]]"));
    }

    /// A spread has no literal value to range-check, so it must fall through to
    /// the conservative answer rather than wave the whole array through.
    #[test]
    fn spread_element_is_not_waved_through() {
        assert!(!accepts(&array_of(TypeTag::I8), "[...other]"));
    }

    fn prop(name: &str, ty: Type, optional: bool) -> ObjectTypeMember {
        ObjectTypeMember::Property {
            name: std::rc::Rc::from(name),
            ty,
            optional,
            readonly: false,
        }
    }

    /// The object-literal arm answers assignability, so an omitted *required*
    /// member must reject — otherwise a `str`-typed field ends up holding null.
    #[test]
    fn object_literal_missing_required_property_is_rejected() {
        let target = Type(
            TypeKind::Object(vec![
                prop("xs", array_of(TypeTag::I8), false),
                prop("name", Type::Str, false),
            ]),
            false,
        );
        assert!(!accepts(&target, "{ xs: [1, 2] }"));
    }

    #[test]
    fn object_literal_may_omit_an_optional_property() {
        let target = Type(
            TypeKind::Object(vec![
                prop("xs", array_of(TypeTag::I8), false),
                prop("name", Type::Str, true),
            ]),
            false,
        );
        assert!(accepts(&target, "{ xs: [1, 2] }"));
        // Still value-checked: an out-of-range element rejects regardless.
        assert!(!accepts(&target, "{ xs: [300] }"));
    }

    /// A required non-narrow property next to the narrow one must not sink the
    /// whole literal, and must still be type-checked.
    #[test]
    fn object_literal_checks_plain_properties_too() {
        let target = Type(
            TypeKind::Object(vec![
                prop("xs", array_of(TypeTag::I8), false),
                prop("name", Type::Str, false),
            ]),
            false,
        );
        assert!(accepts(&target, "{ xs: [1, 2], name: \"ok\" }"));
        assert!(!accepts(&target, "{ xs: [1, 2], name: 42 }"));
        assert!(!accepts(&target, "{ xs: [300], name: \"ok\" }"));
    }
}
