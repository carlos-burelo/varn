mod helpers;

use crate::binder::{BindResult, BindView};
use crate::types::{CheckerTyId, CheckerTyTable, ObjectTypeMember, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::{IntrinsicType, MemberKey, TypeKind};

use self::helpers::{
    class_members_match_object, compatible_named, is_known_named, named_members,
    types_compatible_with_fn_signature,
};

/// Wraps a bare `CheckerTyId` back into an (untainted) `Type` — every
/// recursive field this module reads off `InternedTypeKind` (`Array(T)`,
/// `Fn`'s params/return, `Object`'s member types, ...) is a `CheckerTyId`,
/// not a `Type`, so comparisons that recurse through `types_compatible_impl`
/// (which takes `&Type`) need this at each step.
#[inline]
fn t(id: CheckerTyId) -> Type {
    Type(id, false)
}

fn is_simple_type(ty: &Type, table: &CheckerTyTable) -> bool {
    matches!(table.get(ty.0), TypeKind::Intrinsic(_))
}

fn simple_types_compatible(declared: &Type, inferred: &Type, table: &CheckerTyTable) -> bool {
    match (table.get(declared.0), table.get(inferred.0)) {
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

pub(crate) fn literal_fits_type(target: &Type, int_val: i64, table: &CheckerTyTable) -> bool {
    match table.get(target.0) {
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
fn const_int_value(arena: &varn_core::ast::AstArena, expr: varn_core::ast::ExprId) -> Option<i64> {
    use varn_core::ast::{ExprKind, UnaryOp};
    match &arena.expr(expr).kind {
        ExprKind::IntLiteral { value, .. } => Some(*value),
        ExprKind::Paren { expression } => const_int_value(arena, *expression),
        ExprKind::Unary {
            op, prefix: true, operand, ..
        } => match op {
            UnaryOp::Minus => const_int_value(arena, *operand).and_then(|v| v.checked_neg()),
            UnaryOp::Plus => const_int_value(arena, *operand),
            _ => None,
        },
        _ => None,
    }
}

fn const_float_value(arena: &varn_core::ast::AstArena, expr: varn_core::ast::ExprId) -> Option<f64> {
    use varn_core::ast::{ExprKind, UnaryOp};
    match &arena.expr(expr).kind {
        ExprKind::FloatLiteral { value, .. } => Some(*value),
        ExprKind::IntLiteral { value, .. } => Some(*value as f64),
        ExprKind::Paren { expression } => const_float_value(arena, *expression),
        ExprKind::Unary {
            op, prefix: true, operand, ..
        } => match op {
            UnaryOp::Minus => const_float_value(arena, *operand).map(|v| -v),
            UnaryOp::Plus => const_float_value(arena, *operand),
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
fn plain_literal_matches(
    target: &Type,
    arena: &varn_core::ast::AstArena,
    expr: varn_core::ast::ExprId,
    table: &CheckerTyTable,
) -> bool {
    use varn_core::ast::ExprKind;
    use varn_core::TypeTag;
    let TypeKind::Intrinsic(tag) = table.get(target.0) else {
        return false;
    };
    matches!(
        (&arena.expr(expr).kind, tag),
        (ExprKind::StrLiteral { .. }, TypeTag::Str)
            | (ExprKind::BoolLiteral { .. }, TypeTag::Bool)
            | (ExprKind::CharLiteral { .. }, TypeTag::Char)
            | (ExprKind::IntLiteral { .. }, TypeTag::Int)
            | (ExprKind::FloatLiteral { .. }, TypeTag::Float)
    )
}

fn array_element_type(
    ty: &Type,
    table: &CheckerTyTable,
    interner: Option<&varn_core::AtomInterner>,
) -> Option<Type> {
    match table.get(ty.0) {
        TypeKind::Array(inner) => Some(Type(*inner, false)),
        TypeKind::Generic(name, args, _)
            if table.get_list(*args).len() == 1
                && interner.is_some_and(|it| it.resolve(*name) == IntrinsicType::Array.as_str()) =>
        {
            Some(Type(table.get_list(*args)[0], false))
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
    arena: &varn_core::ast::AstArena,
    expr: Option<varn_core::ast::ExprId>,
    table: &CheckerTyTable,
    interner: Option<&varn_core::AtomInterner>,
) -> bool {
    let Some(expr) = expr else {
        return false;
    };
    use varn_core::ast::{ArrayEl, ExprKind};
    let expr_kind = &arena.expr(expr).kind;
    if let ExprKind::Paren { expression } = expr_kind {
        return expr_satisfies_target_type(
            target_ty,
            _init_ty,
            arena,
            Some(*expression),
            table,
            interner,
        );
    }
    if target_ty.is_granular_int() {
        if let Some(value) = const_int_value(arena, expr) {
            return literal_fits_type(target_ty, value, table);
        }
    }
    if matches!(table.get(target_ty.0), TypeKind::Intrinsic(varn_core::TypeTag::F32)) {
        if let Some(value) = const_float_value(arena, expr) {
            return float_literal_fits_f32(value);
        }
    }
    if let (Some(elem_ty), ExprKind::Array { elements }) =
        (array_element_type(target_ty, table, interner), expr_kind)
    {
        // `Array<Array<i8>>` recurses: the gate asks whether the element type is
        // narrow *or another array*, so nesting does not bail out one level in.
        let narrow_elem = elem_ty.is_granular_int()
            || matches!(table.get(elem_ty.0), TypeKind::Intrinsic(varn_core::TypeTag::F32))
            || array_element_type(&elem_ty, table, interner).is_some();
        if narrow_elem && !elements.is_empty() {
            return elements.iter().all(|el| match el {
                ArrayEl::Expr(e) => {
                    expr_satisfies_target_type(&elem_ty, &elem_ty, arena, Some(*e), table, interner)
                }
                _ => false,
            });
        }
    }
    // An inline object type carries its members inline, so `{ xs: [1, 2] }`
    // against `{ xs: Array<i8> }` can recurse without a binder. A `Named`
    // interface cannot — resolving its members needs a `BindView` this pure
    // function has no access to — so that spelling still falls through.
    if let (TypeKind::Object(mid), ExprKind::Object { properties }) =
        (table.get(target_ty.0), expr_kind)
    {
        let members = table.get_object_members(*mid);
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
                    let ty = Type(*ty, false);
                    expr_satisfies_target_type(&ty, &ty, arena, Some(*value), table, interner)
                        || plain_literal_matches(&ty, arena, *value, table)
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

pub(crate) fn types_compatible(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindView>,
    table: &CheckerTyTable,
) -> bool {
    // `a.0 == b.0` (same `CheckerTyId`) is an O(1) fast path a recursive tree
    // comparison never had: hash-consing guarantees the same shape always
    // gets the same id, so identical ids mean identical types without
    // walking anything. `declared == inferred` below (which also compares
    // the `tainted` bool) already implies this when it holds, but this
    // catches the common "same shape, different taint" case before paying
    // for a cache allocation.
    if declared.0 == inferred.0 {
        return true;
    }
    let mut cache = FxHashMap::default();
    types_compatible_with_cache(declared, inferred, bind, &mut cache, table)
}

pub(crate) fn types_compatible_with_cache(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    table: &CheckerTyTable,
) -> bool {
    if declared.0 == inferred.0 {
        return true;
    }
    if declared.is_dynamic() || inferred.is_dynamic() {
        return true;
    }
    if declared == inferred {
        return true;
    }
    if is_simple_type(declared, table) && is_simple_type(inferred, table) {
        return simple_types_compatible(declared, inferred, table);
    }
    let mut in_progress = FxHashSet::default();
    types_compatible_impl(declared, inferred, bind, cache, &mut in_progress, table)
}

pub(super) fn types_compatible_impl(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
    if declared.0 == inferred.0 {
        return true;
    }
    if declared.is_dynamic() || inferred.is_dynamic() {
        return true;
    }
    if declared == inferred {
        return true;
    }
    if is_simple_type(declared, table) && is_simple_type(inferred, table) {
        return simple_types_compatible(declared, inferred, table);
    }
    let key = (
        *declared,
        *inferred,
        bind.map_or(0usize, |b| b.bind as *const BindResult as usize),
    );
    if let Some(cached) = cache.get(&key) {
        return *cached;
    }
    if !in_progress.insert(key) {
        return true;
    }

    let result = match (table.get(declared.0).clone(), table.get(inferred.0).clone()) {
        (TypeKind::Intrinsic(varn_core::TypeTag::Dynamic), _)
        | (_, TypeKind::Intrinsic(varn_core::TypeTag::Dynamic)) => true,

        (a, b) if a == b => true,

        (TypeKind::Intrinsic(_), TypeKind::Intrinsic(_)) => {
            simple_types_compatible(declared, inferred, table)
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::Str), TypeKind::TemplateLiteral(_)) => true,
        (TypeKind::TemplateLiteral(a), TypeKind::TemplateLiteral(b)) => a == b,

        (TypeKind::Array(_), TypeKind::Array(inf_elem)) if t(inf_elem).is_dynamic() => true,

        (TypeKind::Array(decl_elem), TypeKind::Array(inf_elem)) => {
            types_compatible_impl(&t(decl_elem), &t(inf_elem), bind, cache, in_progress, table)
        }

        (TypeKind::Generic(name, args, _origin), TypeKind::Array(inner)) => {
            let list = table.get_list(args);
            if is_intrinsic(bind, name, IntrinsicType::Array) && list.len() == 1 {
                if t(inner).is_dynamic() {
                    true
                } else {
                    types_compatible_impl(&t(list[0]), &t(inner), bind, cache, in_progress, table)
                }
            } else {
                false
            }
        }
        (TypeKind::Array(inner), TypeKind::Generic(name, args, _origin)) => {
            let list = table.get_list(args);
            if is_intrinsic(bind, name, IntrinsicType::Array) && list.len() == 1 {
                types_compatible_impl(&t(inner), &t(list[0]), bind, cache, in_progress, table)
            } else {
                false
            }
        }

        (TypeKind::Generic(n1, a1, _o1), TypeKind::Generic(n2, a2, _o2)) => {
            let l1 = table.get_list(a1);
            let l2 = table.get_list(a2);
            if is_intrinsic(bind, n1, IntrinsicType::Array)
                && is_intrinsic(bind, n2, IntrinsicType::Array)
                && l1.len() == 1
                && l2.len() == 1
            {
                types_compatible_impl(&t(l1[0]), &t(l2[0]), bind, cache, in_progress, table)
            } else if n1 == n2 {
                l1.len() == l2.len()
                    && l1
                        .to_vec()
                        .iter()
                        .zip(l2.to_vec().iter())
                        .all(|(x, y)| types_compatible_impl(&t(*x), &t(*y), bind, cache, in_progress, table))
            } else {
                false
            }
        }

        (TypeKind::Union(decl_members), TypeKind::Union(inf_members)) => {
            let decl_ids = table.get_list(decl_members).to_vec();
            let inf_ids = table.get_list(inf_members).to_vec();
            inf_ids.iter().all(|im| {
                decl_ids
                    .iter()
                    .any(|dm| types_compatible_impl(&t(*dm), &t(*im), bind, cache, in_progress, table))
            })
        }
        (TypeKind::Union(members), _) => table.get_list(members).to_vec().iter().any(|m| {
            types_compatible_impl(&t(*m), inferred, bind, cache, in_progress, table)
        }),
        (_, TypeKind::Union(inf_members)) => table.get_list(inf_members).to_vec().iter().all(|m| {
            types_compatible_impl(declared, &t(*m), bind, cache, in_progress, table)
        }),
        (_, TypeKind::Intrinsic(varn_core::TypeTag::Never)) => true,

        // Some intrinsics (`str`, `Error`, …) are also nameable declarations, so
        // the same type reaches here spelled two ways: an annotation resolves to
        // `Intrinsic(tag)` while `new Error(…)` infers `Named("Error")` from the
        // class symbol. One spelling, one type. Restricted to the bare `Named`
        // form on purpose — a `Generic` spelling carries type arguments the
        // intrinsic side has nothing to check against.
        (TypeKind::Intrinsic(tag), TypeKind::Named(name, _))
        | (TypeKind::Named(name, _), TypeKind::Intrinsic(tag))
            if resolve_atom(bind, name)
                .as_deref()
                .and_then(IntrinsicType::from_str)
                .is_some_and(|it| it.0 == tag) =>
        {
            true
        }

        (TypeKind::Named(dn, origin_d), TypeKind::Named(in_, origin_i))
        | (TypeKind::Named(dn, origin_d), TypeKind::Generic(in_, _, origin_i))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Named(in_, origin_i))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Generic(in_, _, origin_i)) => {
            match (resolve_atom(bind, dn), resolve_atom(bind, in_)) {
                (Some(dn_s), Some(in_s)) => compatible_named(
                    &dn_s,
                    origin_d
                        .and_then(|o| resolve_atom(bind, o))
                        .as_deref(),
                    &in_s,
                    origin_i
                        .and_then(|o| resolve_atom(bind, o))
                        .as_deref(),
                    bind,
                    cache,
                    in_progress,
                    table,
                ),
                _ => dn == in_,
            }
        }
        (TypeKind::Named(dn, origin_d), TypeKind::Fn(ft))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Fn(ft)) => {
            let (Some(bind), Some(dn_s)) = (bind, resolve_atom(bind, dn)) else {
                return true;
            };
            let origin_d_s = origin_d.and_then(|o| resolve_atom(Some(bind), o));
            let _ = ft;
            if let Some(members) = named_members(bind, &dn_s, origin_d_s.as_deref()) {
                if let Some(callable) = members
                    .iter()
                    .find(|m| m.name.as_ref() == MemberKey::Callable.as_str())
                {
                    return types_compatible_impl(
                        &m_ty(callable),
                        inferred,
                        Some(bind),
                        cache,
                        in_progress,
                        table,
                    );
                }
            }
            true
        }
        (TypeKind::Generic(dn, args, _), TypeKind::Object(inf_fields)) => {
            let arg_ids = table.get_list(args).to_vec();
            if !is_intrinsic(bind, dn, IntrinsicType::Map) || !(arg_ids.len() == 1 || arg_ids.len() == 2) {
                false
            } else {
                let (key_ty, val_ty) = if arg_ids.len() == 2 {
                    (t(arg_ids[0]), t(arg_ids[1]))
                } else {
                    (Type::Str, t(arg_ids[0]))
                };
                let key_compat = types_compatible_impl(&key_ty, &Type::Str, bind, cache, in_progress, table);
                if !key_compat {
                    false
                } else {
                    table.get_object_members(inf_fields).iter().all(|im| match im {
                        ObjectTypeMember::Property { ty, .. } => {
                            types_compatible_impl(&val_ty, &t(*ty), bind, cache, in_progress, table)
                        }
                        ObjectTypeMember::Index {
                            key_ty: ik,
                            value_ty: iv,
                            ..
                        } => {
                            types_compatible_impl(&key_ty, &t(*ik), bind, cache, in_progress, table)
                                && types_compatible_impl(&val_ty, &t(*iv), bind, cache, in_progress, table)
                        }
                        _ => false,
                    })
                }
            }
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::Map), TypeKind::Object(_))
        | (TypeKind::Object(_), TypeKind::Intrinsic(varn_core::TypeTag::Map)) => true,
        (TypeKind::Named(dn, origin_d), TypeKind::Object(inf_fields))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Object(inf_fields)) => {
            if is_intrinsic(bind, dn, IntrinsicType::Map) {
                true
            } else if let (Some(bind), Some(dn_s)) = (bind, resolve_atom(bind, dn)) {
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
            }
        }
        (TypeKind::Object(decl_fields), TypeKind::Generic(in_, args, _)) => {
            let arg_ids = table.get_list(args).to_vec();
            if !is_intrinsic(bind, in_, IntrinsicType::Map) || !(arg_ids.len() == 1 || arg_ids.len() == 2) {
                false
            } else {
                let (key_ty, val_ty) = if arg_ids.len() == 2 {
                    (t(arg_ids[0]), t(arg_ids[1]))
                } else {
                    (Type::Str, t(arg_ids[0]))
                };
                table.get_object_members(decl_fields).iter().all(|dm| match dm {
                    ObjectTypeMember::Index {
                        key_ty: dk,
                        value_ty: dv,
                        ..
                    } => {
                        types_compatible_impl(&t(*dk), &key_ty, bind, cache, in_progress, table)
                            && types_compatible_impl(&t(*dv), &val_ty, bind, cache, in_progress, table)
                    }
                    ObjectTypeMember::Property { optional: true, .. } => true,
                    _ => false,
                })
            }
        }
        (TypeKind::Object(decl_fields), TypeKind::Named(in_, origin_i))
        | (TypeKind::Object(decl_fields), TypeKind::Generic(in_, _, origin_i)) => {
            if is_intrinsic(bind, in_, IntrinsicType::Map) {
                true
            } else if let (Some(bind), Some(in_s)) = (bind, resolve_atom(bind, in_)) {
                let origin_i_s = origin_i.and_then(|o| resolve_atom(Some(bind), o));
                if let Some(inf_members) = named_members(bind, &in_s, origin_i_s.as_deref()) {
                    crate::checker::compat::helpers::object_matches_class_members(
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
            }
        }
        (TypeKind::Named(dn, _), _) => {
            if is_intrinsic(bind, dn, IntrinsicType::Map) {
                true
            } else {
                named_fallback(declared, inferred, dn, None, bind, cache, in_progress, table, true)
            }
        }
        (_, TypeKind::Named(in_, _)) => {
            if is_intrinsic(bind, in_, IntrinsicType::Map) {
                true
            } else {
                named_fallback(declared, inferred, in_, None, bind, cache, in_progress, table, false)
            }
        }
        (TypeKind::Generic(name, args, _origin), _) => {
            let list = table.get_list(args);
            if is_intrinsic(bind, name, IntrinsicType::Task) && list.len() == 1 {
                types_compatible_impl(&t(list[0]), inferred, bind, cache, in_progress, table)
            } else {
                false
            }
        }
        (TypeKind::Fn(fid1), TypeKind::Fn(fid2)) => {
            let ft1 = table.get_function(fid1).clone();
            let ft2 = table.get_function(fid2).clone();
            let return_ok = t(ft2.return_type).is_dynamic()
                || matches!(table.get(ft1.return_type), TypeKind::Intrinsic(varn_core::TypeTag::Void))
                || types_compatible_impl(
                    &t(ft1.return_type),
                    &t(ft2.return_type),
                    bind,
                    cache,
                    in_progress,
                    table,
                );
            ft2.params.len() <= ft1.params.len()
                && return_ok
                && ft1.params.iter().zip(ft2.params.iter()).all(|(t1, t2)| {
                    t(t2.ty).is_dynamic()
                        || matches!(table.get(t2.ty), TypeKind::Named(_, _))
                        || (types_compatible_impl(&t(t2.ty), &t(t1.ty), bind, cache, in_progress, table)
                            && t1.optional == t2.optional)
                })
        }

        (TypeKind::Object(decl_fields), TypeKind::Object(inf_fields)) => {
            let decl_fields = table.get_object_members(decl_fields).to_vec();
            let inf_fields = table.get_object_members(inf_fields).to_vec();
            let mut ok = true;
            'outer: for dm in &decl_fields {
                match dm {
                    ObjectTypeMember::Property {
                        name, ty, optional, ..
                    } => {
                        let found = inf_fields.iter().find_map(|im| match im {
                            ObjectTypeMember::Property {
                                name: iname,
                                ty: ity,
                                ..
                            } if iname == name => Some(*ity),
                            _ => None,
                        });
                        match found {
                            Some(inf_ty) => {
                                if !types_compatible_impl(&t(*ty), &t(inf_ty), bind, cache, in_progress, table) {
                                    ok = false;
                                    break 'outer;
                                }
                            }
                            None if !*optional => {
                                ok = false;
                                break 'outer;
                            }
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
                            } if iname == name => Some((p2.clone(), *r2, *o2)),
                            _ => None,
                        });
                        match found {
                            Some((p2, r2, o2)) => {
                                if *optional != o2
                                    || !types_compatible_impl(&t(*r1), &t(r2), bind, cache, in_progress, table)
                                    || p1.len() != p2.len()
                                    || p1.iter().zip(p2.iter()).any(|(t1, t2)| {
                                        !types_compatible_impl(
                                            &t(t1.ty),
                                            &t(t2.ty),
                                            bind,
                                            cache,
                                            in_progress,
                                            table,
                                        ) || t1.optional != t2.optional
                                    })
                                {
                                    ok = false;
                                    break 'outer;
                                }
                            }
                            None => {
                                ok = false;
                                break 'outer;
                            }
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
                                types_compatible_impl(&t(*key_ty), &t(*ikey), bind, cache, in_progress, table)
                                    && types_compatible_impl(
                                        &t(*value_ty),
                                        &t(*ivalue),
                                        bind,
                                        cache,
                                        in_progress,
                                        table,
                                    )
                            }
                            _ => false,
                        });
                        if has_compatible_index {
                            continue;
                        }

                        let explicit_members_compatible = inf_fields.iter().all(|im| match im {
                            ObjectTypeMember::Property { ty, .. } => {
                                types_compatible_impl(&t(*value_ty), &t(*ty), bind, cache, in_progress, table)
                            }
                            ObjectTypeMember::Method {
                                params,
                                return_type,
                                is_arrow,
                                ..
                            } => types_compatible_with_fn_signature(
                                &t(*value_ty),
                                params,
                                *return_type,
                                *is_arrow,
                                bind,
                                cache,
                                in_progress,
                                table,
                            ),
                            _ => true,
                        });
                        if !explicit_members_compatible {
                            ok = false;
                            break 'outer;
                        }
                    }
                    _ => {}
                }
            }

            if ok {
                let has_index_decl = decl_fields
                    .iter()
                    .any(|m| matches!(m, ObjectTypeMember::Index { .. }));
                if !has_index_decl {
                    for im in &inf_fields {
                        if let ObjectTypeMember::Property { name: iname, .. } = im {
                            let exists_in_decl = decl_fields.iter().any(|dm| match dm {
                                ObjectTypeMember::Property { name: dname, .. } => dname == iname,
                                ObjectTypeMember::Method { name: dname, .. } => dname == iname,
                                _ => false,
                            });
                            if !exists_in_decl {
                                ok = false;
                                break;
                            }
                        }
                    }
                }
            }
            ok
        }

        (TypeKind::Tuple(decl_elems), TypeKind::Array(inf_elem)) => table
            .get_list(decl_elems)
            .to_vec()
            .iter()
            .all(|d| types_compatible_impl(&t(*d), &t(inf_elem), bind, cache, in_progress, table)),

        (TypeKind::Tuple(decl_elems), TypeKind::Tuple(inf_elems)) => {
            let decl_ids = table.get_list(decl_elems).to_vec();
            let inf_ids = table.get_list(inf_elems).to_vec();
            decl_ids.len() == inf_ids.len()
                && decl_ids
                    .iter()
                    .zip(inf_ids.iter())
                    .all(|(d, i)| types_compatible_impl(&t(*d), &t(*i), bind, cache, in_progress, table))
        }

        (TypeKind::Intersection(decl_members), _) => table.get_list(decl_members).to_vec().iter().all(|m| {
            types_compatible_impl(&t(*m), inferred, bind, cache, in_progress, table)
        }),

        (_, TypeKind::Intersection(inf_members)) => table.get_list(inf_members).to_vec().iter().any(|m| {
            types_compatible_impl(declared, &t(*m), bind, cache, in_progress, table)
        }),

        _ => false,
    };

    in_progress.remove(&key);
    cache.insert(key, result);
    result
}

/// `Named`/`Fn`-mismatch fallback: try expanding `name` as a type alias and
/// retry, same as the old code's `(Named, _)`/`(_, Named)` arms. `is_declared`
/// picks which side `name`/`origin` belong to.
#[allow(clippy::too_many_arguments)]
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
    let Some(expanded) =
        bind.resolve_type_alias(&name_s, origin_s.as_deref())
    else {
        return false;
    };
    if is_declared {
        if expanded.0 != declared.0 {
            return types_compatible_impl(&expanded, inferred, Some(bind), cache, in_progress, table);
        }
    } else if expanded.0 != inferred.0 {
        return types_compatible_impl(declared, &expanded, Some(bind), cache, in_progress, table);
    }
    false
}

fn ctx_interner<'a>(bind: Option<&'a BindView>) -> Option<&'a varn_core::AtomInterner> {
    bind.map(|b| &b.bind.interner)
}

/// Resolve `atom` to owned text without panicking on cross-module staleness.
///
/// `TypeKind::Named`/`Generic` names are `Atom`s minted by whichever module's
/// binder built the type; `bind`'s own snapshot can be behind a sibling
/// module that published more atoms after this bind was snapshotted (the
/// `compile_stdlib_bundle` loop binds dozens of modules against one live
/// table). The bind's table comes first (fast path, no clone); the
/// resolver's live snapshot is the fallback (same prefix guarantee, so a hit
/// there is never wrong). `None` when neither table knows the atom — callers
/// degrade conservatively instead of indexing out of bounds.
fn resolve_atom(bind: Option<&BindView>, atom: varn_core::Atom) -> Option<String> {
    let b = bind?;
    if let Some(s) = b.bind.interner.try_resolve(atom) {
        return Some(s.to_string());
    }
    b.resolver
        .interner_snapshot()
        .try_resolve(atom)
        .map(|s| s.to_string())
}

/// `true` when `atom` names the intrinsic `intrinsic`, without resolving
/// `atom` itself: a non-panicking lookup of the *known* side, so a foreign
/// (or not-yet-published) atom simply doesn't match instead of crashing.
fn is_intrinsic(
    bind: Option<&BindView>,
    atom: varn_core::Atom,
    intrinsic: IntrinsicType,
) -> bool {
    ctx_interner(bind).is_some_and(|i| i.get(intrinsic.as_str()) == Some(atom))
}

fn m_ty(m: &crate::types::ClassMemberInfo) -> Type {
    m.ty
}

#[cfg(test)]
mod tests {
    use super::*;
    use varn_core::TypeTag;

    /// Parses `let probe = <src>` and hands back the arena plus the
    /// initializer's `ExprId`. `expr_satisfies_target_type` is a pure
    /// function of (target type, expr), so a real parse is all the fixture
    /// the value-level checks need — no binder, no scopes.
    fn init_of(src: &str) -> (varn_core::ast::AstArena, varn_core::ast::ExprId) {
        let text = format!("let probe = {src}\n");
        let (tokens, lexemes, _) = varn_lexer::scan(&text, "compat-test");
        let (program, _interner, arena) = varn_parser::parse(
            tokens,
            lexemes,
            "compat-test",
            varn_core::AtomInterner::new(),
        )
        .expect("parses");
        for stmt in &program.body {
            if let varn_core::ast::StmtKind::Decl(decl) = &arena.stmt(*stmt).kind {
                if let varn_core::ast::Decl::Variable(v) = decl.as_ref() {
                    if let Some(init) = v.declarators[0].init {
                        return (arena, init);
                    }
                }
            }
        }
        panic!("no initializer parsed from `{src}`");
    }

    fn accepts(target: &Type, table: &CheckerTyTable, src: &str) -> bool {
        let (arena, init) = init_of(src);
        expr_satisfies_target_type(target, &Type::Dynamic, &arena, Some(init), table, None)
    }

    fn array_of(tag: TypeTag, table: &mut CheckerTyTable) -> Type {
        let elem = Type::intrinsic(tag, table);
        Type::array(elem, table)
    }

    /// The safety property a later compact `ArrayRepr` depends on: an element
    /// that does not fit the declared narrow width must never be accepted, or it
    /// would be truncated silently at runtime instead.
    #[test]
    fn narrow_array_literal_rejects_out_of_range_element() {
        let mut table = CheckerTyTable::default();
        let i8_arr = array_of(TypeTag::I8, &mut table);
        let u8_arr = array_of(TypeTag::U8, &mut table);
        let u32_arr = array_of(TypeTag::U32, &mut table);
        assert!(!accepts(&i8_arr, &table, "[300]"));
        assert!(!accepts(&i8_arr, &table, "[0, 1, -129]"));
        assert!(!accepts(&u8_arr, &table, "[-1]"));
        assert!(!accepts(&u32_arr, &table, "[4294967296]"));
    }

    #[test]
    fn narrow_array_literal_accepts_in_range_elements() {
        let mut table = CheckerTyTable::default();
        let i8_arr = array_of(TypeTag::I8, &mut table);
        let u8_arr = array_of(TypeTag::U8, &mut table);
        let u32_arr = array_of(TypeTag::U32, &mut table);
        assert!(accepts(&i8_arr, &table, "[-128, 0, 127]"));
        assert!(accepts(&u8_arr, &table, "[0, 255]"));
        assert!(accepts(&u32_arr, &table, "[0, 4000000000]"));
    }

    #[test]
    fn f32_literal_that_would_narrow_to_infinity_is_rejected() {
        let mut table = CheckerTyTable::default();
        let f32_arr = array_of(TypeTag::F32, &mut table);
        let f32_scalar = Type::intrinsic(TypeTag::F32, &mut table);
        assert!(!accepts(&f32_arr, &table, "[1e300]"));
        assert!(!accepts(&f32_scalar, &table, "1e300"));
        assert!(accepts(&f32_arr, &table, "[1.5, -2.25, 3]"));
        assert!(accepts(&f32_scalar, &table, "3.14"));
    }

    #[test]
    fn nested_narrow_array_literals_recurse() {
        let mut table = CheckerTyTable::default();
        let i8_arr = array_of(TypeTag::I8, &mut table);
        let nested = Type::array(i8_arr, &mut table);
        assert!(accepts(&nested, &table, "[[1, 2], [3]]"));
        assert!(!accepts(&nested, &table, "[[1, 2], [300]]"));
    }

    /// A spread has no literal value to range-check, so it must fall through to
    /// the conservative answer rather than wave the whole array through.
    #[test]
    fn spread_element_is_not_waved_through() {
        let mut table = CheckerTyTable::default();
        let i8_arr = array_of(TypeTag::I8, &mut table);
        assert!(!accepts(&i8_arr, &table, "[...other]"));
    }

    fn prop(name: &str, ty: Type, optional: bool) -> ObjectTypeMember {
        ObjectTypeMember::Property {
            name: std::rc::Rc::from(name),
            ty: ty.0,
            optional,
            readonly: false,
        }
    }

    /// The object-literal arm answers assignability, so an omitted *required*
    /// member must reject — otherwise a `str`-typed field ends up holding null.
    #[test]
    fn object_literal_missing_required_property_is_rejected() {
        let mut table = CheckerTyTable::default();
        let i8_arr = array_of(TypeTag::I8, &mut table);
        let members = table.intern_object_members(vec![
            prop("xs", i8_arr, false),
            prop("name", Type::Str, false),
        ]);
        let target = Type(table.intern(TypeKind::Object(members)), false);
        assert!(!accepts(&target, &table, "{ xs: [1, 2] }"));
    }

    #[test]
    fn object_literal_may_omit_an_optional_property() {
        let mut table = CheckerTyTable::default();
        let i8_arr = array_of(TypeTag::I8, &mut table);
        let members = table.intern_object_members(vec![
            prop("xs", i8_arr, false),
            prop("name", Type::Str, true),
        ]);
        let target = Type(table.intern(TypeKind::Object(members)), false);
        assert!(accepts(&target, &table, "{ xs: [1, 2] }"));
        // Still value-checked: an out-of-range element rejects regardless.
        assert!(!accepts(&target, &table, "{ xs: [300] }"));
    }

    /// A required non-narrow property next to the narrow one must not sink the
    /// whole literal, and must still be type-checked.
    #[test]
    fn object_literal_checks_plain_properties_too() {
        let mut table = CheckerTyTable::default();
        let i8_arr = array_of(TypeTag::I8, &mut table);
        let members = table.intern_object_members(vec![
            prop("xs", i8_arr, false),
            prop("name", Type::Str, false),
        ]);
        let target = Type(table.intern(TypeKind::Object(members)), false);
        assert!(accepts(&target, &table, "{ xs: [1, 2], name: \"ok\" }"));
        assert!(!accepts(&target, &table, "{ xs: [1, 2], name: 42 }"));
        assert!(!accepts(&target, &table, "{ xs: [300], name: \"ok\" }"));
    }
}
