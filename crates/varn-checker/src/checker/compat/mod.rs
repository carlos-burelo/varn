mod helpers;

use crate::binder::{BindResult, BindView};
use crate::types::{CheckerTyId, CheckerTyTable, ObjectTypeMember, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use varn_core::{MemberKey, TypeKind};

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
    matches!(table.get(ty.0), TypeKind::Primitive(_) | TypeKind::Builtin(_))
}

/// Scalar assignability. Only `int` widens implicitly, and only into the
/// exact domains `decimal` and `bigint` (spec §9, ADR-0015 D4).
fn simple_types_compatible(declared: &Type, inferred: &Type, table: &CheckerTyTable) -> bool {
    use varn_core::LangPrimitive as P;
    match (table.get(declared.0), table.get(inferred.0)) {
        (TypeKind::Primitive(P::Dynamic), _) | (_, TypeKind::Primitive(P::Dynamic)) => true,
        (a, b) if a == b => true,
        (_, TypeKind::Primitive(P::Never)) => true,
        (TypeKind::Primitive(P::Decimal | P::BigInt), TypeKind::Primitive(P::Int)) => true,
        _ => false,
    }
}

use crate::types::numeric_literal::{const_int_value, int_literal_adopts};

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
    use varn_core::LangPrimitive as P;
    let TypeKind::Primitive(p) = table.get(target.0) else {
        return false;
    };
    matches!(
        (&arena.expr(expr).kind, p),
        (ExprKind::StrLiteral { .. }, P::Str)
            | (ExprKind::BoolLiteral { .. }, P::Bool)
            | (ExprKind::CharLiteral { .. }, P::Char)
            | (ExprKind::IntLiteral { .. }, P::Int)
            | (ExprKind::FloatLiteral { .. }, P::Float)
    )
}

/// Whether the literal expression `expr` is a value of the literal type
/// `target` — directly or as a member of a union (`"GET" | "POST"`, `200?`).
fn literal_expr_admitted(
    target: &Type,
    arena: &varn_core::ast::AstArena,
    expr: varn_core::ast::ExprId,
    table: &CheckerTyTable,
    interner: Option<&varn_core::AtomInterner>,
) -> bool {
    use varn_core::ast::ExprKind;
    use varn_core::TypeLiteral;
    let is_value = |lit: TypeLiteral<varn_core::Atom>| match (lit, &arena.expr(expr).kind) {
        (TypeLiteral::Str(atom), ExprKind::StrLiteral { value }) => {
            interner.and_then(|i| i.try_resolve(atom)) == Some(value.as_str())
        }
        (TypeLiteral::Bool(b), ExprKind::BoolLiteral { value }) => b == *value,
        (TypeLiteral::Char(c), ExprKind::CharLiteral { value }) => c == *value,
        (TypeLiteral::Int(v), _) => const_int_value(arena, expr) == Some(v),
        _ => false,
    };
    match table.get(target.0) {
        TypeKind::Literal(l) => is_value(l),
        TypeKind::Union(list) => table.get_list(list).iter().any(|m| match table.get(*m) {
            TypeKind::Literal(l) => is_value(l),
            _ => false,
        }),
        _ => false,
    }
}

fn array_element_type(
    ty: &Type,
    table: &CheckerTyTable,
    interner: Option<&varn_core::AtomInterner>,
) -> Option<Type> {
    match table.get(ty.0) {
        TypeKind::Array(inner) => Some(Type(inner, false)),
        TypeKind::Generic(name, args, _)
            if table.get_list(args).len() == 1
                && interner.is_some_and(|it| it.resolve(name) == varn_core::BuiltinType::Array.name()) =>
        {
            Some(Type(table.get_list(args)[0], false))
        }
        _ => None,
    }
}

/// Assignability escape hatch for *literals* whose value, not their type,
/// makes them fit: `int` is not a subtype of `float`, but the literal `42`
/// adopts `float` because `f64` holds it exactly (spec §5). Array and inline
/// object literals recurse, so `[1, 2]` fits `float[]` while
/// `[9007199254740993]` does not. `value_assignable_to` pairs this with
/// `types_compatible`.
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
    if let Some(value) = const_int_value(arena, expr) {
        if int_literal_adopts(target_ty, value, table) {
            return true;
        }
    }
    if literal_expr_admitted(target_ty, arena, expr, table, interner) {
        return true;
    }
    if let (Some(elem_ty), ExprKind::Array { elements }) =
        (array_element_type(target_ty, table, interner), expr_kind)
    {
        // `Array<Array<i8>>` recurses: the gate asks whether the element type is
        // narrow *or another array*, so nesting does not bail out one level in.
        let literal_elem = matches!(
                table.get(elem_ty.0),
                TypeKind::Primitive(
                    varn_core::LangPrimitive::Float
                        | varn_core::LangPrimitive::Decimal
                        | varn_core::LangPrimitive::BigInt
                )
            )
            || array_element_type(&elem_ty, table, interner).is_some();
        if literal_elem && !elements.is_empty() {
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
        let members = table.get_object_members(mid);
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
        (TypeKind::Primitive(varn_core::LangPrimitive::Dynamic), _)
        | (_, TypeKind::Primitive(varn_core::LangPrimitive::Dynamic)) => true,

        (a, b) if a == b => true,

        (
            TypeKind::Primitive(_) | TypeKind::Builtin(_),
            TypeKind::Primitive(_) | TypeKind::Builtin(_),
        ) => {
            simple_types_compatible(declared, inferred, table)
        }
        (TypeKind::Primitive(p), TypeKind::Literal(l)) => {
            use varn_core::LangPrimitive as P;
            let base = l.base();
            p == base || (matches!(p, P::Decimal | P::BigInt) && base == P::Int)
        }
        (TypeKind::Primitive(varn_core::LangPrimitive::Str), TypeKind::TemplateLiteral(_)) => true,
        (TypeKind::TemplateLiteral(a), TypeKind::TemplateLiteral(b)) => a == b,

        (TypeKind::Array(_), TypeKind::Array(inf_elem)) if t(inf_elem).is_dynamic() => true,

        (TypeKind::Array(decl_elem), TypeKind::Array(inf_elem)) => {
            types_compatible_impl(&t(decl_elem), &t(inf_elem), bind, cache, in_progress, table)
        }

        (TypeKind::Generic(name, args, _origin), TypeKind::Array(inner)) => {
            let list = table.get_list(args);
            if is_intrinsic(bind, name, varn_core::BuiltinType::Array.name()) && list.len() == 1 {
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
            if is_intrinsic(bind, name, varn_core::BuiltinType::Array.name()) && list.len() == 1 {
                types_compatible_impl(&t(inner), &t(list[0]), bind, cache, in_progress, table)
            } else {
                false
            }
        }

        (TypeKind::Generic(n1, a1, _o1), TypeKind::Generic(n2, a2, _o2)) => {
            let l1 = table.get_list(a1);
            let l2 = table.get_list(a2);
            if is_intrinsic(bind, n1, varn_core::BuiltinType::Array.name())
                && is_intrinsic(bind, n2, varn_core::BuiltinType::Array.name())
                && l1.len() == 1
                && l2.len() == 1
            {
                types_compatible_impl(&t(l1[0]), &t(l2[0]), bind, cache, in_progress, table)
            } else if n1 == n2 {
                l1.len() == l2.len()
                    && l1.to_vec().iter().zip(l2.to_vec().iter()).all(|(x, y)| {
                        types_compatible_impl(&t(*x), &t(*y), bind, cache, in_progress, table)
                    })
            } else {
                false
            }
        }

        (TypeKind::Union(decl_members), TypeKind::Union(inf_members)) => {
            let decl_ids = table.get_list(decl_members).to_vec();
            let inf_ids = table.get_list(inf_members).to_vec();
            inf_ids.iter().all(|im| {
                decl_ids.iter().any(|dm| {
                    types_compatible_impl(&t(*dm), &t(*im), bind, cache, in_progress, table)
                })
            })
        }
        (TypeKind::Union(members), _) => table
            .get_list(members)
            .to_vec()
            .iter()
            .any(|m| types_compatible_impl(&t(*m), inferred, bind, cache, in_progress, table)),
        (_, TypeKind::Union(inf_members)) => table
            .get_list(inf_members)
            .to_vec()
            .iter()
            .all(|m| types_compatible_impl(declared, &t(*m), bind, cache, in_progress, table)),
        (_, TypeKind::Primitive(varn_core::LangPrimitive::Never)) => true,

        // Some intrinsics (`str`, `Error`, …) are also nameable declarations, so
        // the same type reaches here spelled two ways: an annotation resolves to
        // `Intrinsic(tag)` while `new Error(…)` infers `Named("Error")` from the
        // class symbol. One spelling, one type. Restricted to the bare `Named`
        // form on purpose — a `Generic` spelling carries type arguments the
        // intrinsic side has nothing to check against.
        (lang @ (TypeKind::Primitive(_) | TypeKind::Builtin(_)), TypeKind::Named(name, _))
        | (TypeKind::Named(name, _), lang @ (TypeKind::Primitive(_) | TypeKind::Builtin(_)))
            if resolve_atom(bind, name)
                .as_deref()
                .and_then(TypeKind::of_lang_name)
                .is_some_and(|k| k == lang) =>
        {
            true
        }

        (TypeKind::Named(dn, origin_d), TypeKind::Named(in_, origin_i))
        | (TypeKind::Named(dn, origin_d), TypeKind::Generic(in_, _, origin_i))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Named(in_, origin_i)) => {
            match (resolve_atom(bind, dn), resolve_atom(bind, in_)) {
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
        // `Map<K, V>` is a collection, not an indexable object (spec §24).
        (TypeKind::Generic(..), TypeKind::Object(_)) => false,
        (TypeKind::Named(dn, origin_d), TypeKind::Object(inf_fields)) => {
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
            }
        }
        (TypeKind::Object(_), TypeKind::Generic(..)) => false,
        (TypeKind::Object(decl_fields), TypeKind::Named(in_, origin_i)) => {
            if let (Some(bind), Some(in_s)) = (bind, resolve_atom(bind, in_)) {
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
        (TypeKind::Named(dn, dn_origin), _) => named_fallback(
            declared,
            inferred,
            dn,
            dn_origin,
            bind,
            cache,
            in_progress,
            table,
            true,
        ),
        (_, TypeKind::Named(in_, in_origin)) => named_fallback(
            declared,
            inferred,
            in_,
            in_origin,
            bind,
            cache,
            in_progress,
            table,
            false,
        ),
        (TypeKind::Generic(name, args, _origin), _) => {
            let list = table.get_list(args);
            if is_intrinsic(bind, name, varn_core::BuiltinType::Task.name()) && list.len() == 1 {
                types_compatible_impl(&t(list[0]), inferred, bind, cache, in_progress, table)
            } else {
                false
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
                                if !types_compatible_impl(
                                    &t(*ty),
                                    &t(inf_ty),
                                    bind,
                                    cache,
                                    in_progress,
                                    table,
                                ) {
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
                                    || !types_compatible_impl(
                                        &t(*r1),
                                        &t(r2),
                                        bind,
                                        cache,
                                        in_progress,
                                        table,
                                    )
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
                                types_compatible_impl(
                                    &t(*key_ty),
                                    &t(*ikey),
                                    bind,
                                    cache,
                                    in_progress,
                                    table,
                                ) && types_compatible_impl(
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
                            ObjectTypeMember::Property { ty, .. } => types_compatible_impl(
                                &t(*value_ty),
                                &t(*ty),
                                bind,
                                cache,
                                in_progress,
                                table,
                            ),
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

        (TypeKind::Tuple(decl_elems), TypeKind::Array(inf_elem)) => {
            table.get_list(decl_elems).to_vec().iter().all(|d| {
                types_compatible_impl(&t(*d), &t(inf_elem), bind, cache, in_progress, table)
            })
        }

        (TypeKind::Tuple(decl_elems), TypeKind::Tuple(inf_elems)) => {
            let decl_ids = table.get_list(decl_elems).to_vec();
            let inf_ids = table.get_list(inf_elems).to_vec();
            decl_ids.len() == inf_ids.len()
                && decl_ids.iter().zip(inf_ids.iter()).all(|(d, i)| {
                    types_compatible_impl(&t(*d), &t(*i), bind, cache, in_progress, table)
                })
        }

        (TypeKind::Intersection(decl_members), _) => table
            .get_list(decl_members)
            .to_vec()
            .iter()
            .all(|m| types_compatible_impl(&t(*m), inferred, bind, cache, in_progress, table)),

        (_, TypeKind::Intersection(inf_members)) => table
            .get_list(inf_members)
            .to_vec()
            .iter()
            .any(|m| types_compatible_impl(declared, &t(*m), bind, cache, in_progress, table)),

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
fn is_intrinsic(bind: Option<&BindView>, atom: varn_core::Atom, name: &str) -> bool {
    ctx_interner(bind).is_some_and(|i| i.get(name) == Some(atom))
}

fn m_ty(m: &crate::types::ClassMemberInfo) -> Type {
    m.ty
}

#[cfg(test)]
mod tests {
    use super::*;
    

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

    fn array_of(p: varn_core::LangPrimitive, table: &mut CheckerTyTable) -> Type {
        let elem = Type::primitive(p, table);
        Type::array(elem, table)
    }

    /// An integer literal element adopts `float` only when `f64` holds it
    /// exactly; otherwise the array literal must not be accepted.
    #[test]
    fn float_array_literal_accepts_exact_integers_only() {
        let mut table = CheckerTyTable::default();
        let f_arr = array_of(varn_core::LangPrimitive::Float, &mut table);
        assert!(accepts(&f_arr, &table, "[1, 2, 3]"));
        assert!(accepts(&f_arr, &table, "[-1, 9007199254740992]"));
        assert!(!accepts(&f_arr, &table, "[9007199254740993]"));
    }

    #[test]
    fn nested_literal_arrays_recurse() {
        let mut table = CheckerTyTable::default();
        let f_arr = array_of(varn_core::LangPrimitive::Float, &mut table);
        let nested = Type::array(f_arr, &mut table);
        assert!(accepts(&nested, &table, "[[1, 2], [3]]"));
        assert!(!accepts(&nested, &table, "[[1, 2], [9007199254740993]]"));
    }

    /// A spread has no literal value to check, so it must fall through to the
    /// conservative answer rather than wave the whole array through.
    #[test]
    fn spread_element_is_not_waved_through() {
        let mut table = CheckerTyTable::default();
        let f_arr = array_of(varn_core::LangPrimitive::Float, &mut table);
        assert!(!accepts(&f_arr, &table, "[...other]"));
    }

    fn prop(name: &str, ty: Type, optional: bool) -> ObjectTypeMember {
        ObjectTypeMember::Property {
            name: std::sync::Arc::from(name),
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
        let f_arr = array_of(varn_core::LangPrimitive::Float, &mut table);
        let members = table.intern_object_members(vec![
            prop("xs", f_arr, false),
            prop("name", Type::Str, false),
        ]);
        let target = Type(table.intern(TypeKind::Object(members)), false);
        assert!(!accepts(&target, &table, "{ xs: [1, 2] }"));
    }

    #[test]
    fn object_literal_may_omit_an_optional_property() {
        let mut table = CheckerTyTable::default();
        let f_arr = array_of(varn_core::LangPrimitive::Float, &mut table);
        let members = table.intern_object_members(vec![
            prop("xs", f_arr, false),
            prop("name", Type::Str, true),
        ]);
        let target = Type(table.intern(TypeKind::Object(members)), false);
        assert!(accepts(&target, &table, "{ xs: [1, 2] }"));
        // Still value-checked: an inexact element rejects regardless.
        assert!(!accepts(&target, &table, "{ xs: [9007199254740993] }"));
    }

    /// A required plain property next to the literal-checked one must not sink the
    /// whole literal, and must still be type-checked.
    #[test]
    fn object_literal_checks_plain_properties_too() {
        let mut table = CheckerTyTable::default();
        let f_arr = array_of(varn_core::LangPrimitive::Float, &mut table);
        let members = table.intern_object_members(vec![
            prop("xs", f_arr, false),
            prop("name", Type::Str, false),
        ]);
        let target = Type(table.intern(TypeKind::Object(members)), false);
        assert!(accepts(&target, &table, "{ xs: [1, 2], name: \"ok\" }"));
        assert!(!accepts(&target, &table, "{ xs: [1, 2], name: 42 }"));
        assert!(!accepts(&target, &table, "{ xs: [9007199254740993], name: \"ok\" }"));
    }
}
