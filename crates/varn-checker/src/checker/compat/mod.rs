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
                varn_core::TypeTag::Int
                    | varn_core::TypeTag::I8
                    | varn_core::TypeTag::I16
                    | varn_core::TypeTag::U8
                    | varn_core::TypeTag::U16
            )
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::I16), TypeKind::Intrinsic(inf_tag)) => {
            matches!(
                inf_tag,
                varn_core::TypeTag::Int | varn_core::TypeTag::I8 | varn_core::TypeTag::U8
            )
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::I8), TypeKind::Intrinsic(inf_tag)) => {
            matches!(inf_tag, varn_core::TypeTag::Int)
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::U64), TypeKind::Intrinsic(inf_tag)) => {
            matches!(
                inf_tag,
                varn_core::TypeTag::Int
                    | varn_core::TypeTag::U8
                    | varn_core::TypeTag::U16
                    | varn_core::TypeTag::U32
            )
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::U32), TypeKind::Intrinsic(inf_tag)) => {
            matches!(
                inf_tag,
                varn_core::TypeTag::Int | varn_core::TypeTag::U8 | varn_core::TypeTag::U16
            )
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::U16), TypeKind::Intrinsic(inf_tag)) => {
            matches!(inf_tag, varn_core::TypeTag::Int | varn_core::TypeTag::U8)
        }
        (TypeKind::Intrinsic(varn_core::TypeTag::U8), TypeKind::Intrinsic(inf_tag)) => {
            matches!(inf_tag, varn_core::TypeTag::Int)
        }
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
                varn_core::TypeTag::Float
                    | varn_core::TypeTag::Int
                    | varn_core::TypeTag::I8
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
            let name_s = ctx_interner(bind).map(|i| i.resolve(name));
            let list = table.get_list(args);
            if name_s == Some(IntrinsicType::Array.as_str()) && list.len() == 1 {
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
            let name_s = ctx_interner(bind).map(|i| i.resolve(name));
            let list = table.get_list(args);
            if name_s == Some(IntrinsicType::Array.as_str()) && list.len() == 1 {
                types_compatible_impl(&t(inner), &t(list[0]), bind, cache, in_progress, table)
            } else {
                false
            }
        }

        (TypeKind::Generic(n1, a1, _o1), TypeKind::Generic(n2, a2, _o2)) => {
            let interner = ctx_interner(bind);
            let n1_s = interner.map(|i| i.resolve(n1));
            let n2_s = interner.map(|i| i.resolve(n2));
            let l1 = table.get_list(a1);
            let l2 = table.get_list(a2);
            if n1_s == Some(IntrinsicType::Array.as_str())
                && n2_s == Some(IntrinsicType::Array.as_str())
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
            if ctx_interner(bind)
                .map(|i| i.resolve(name))
                .and_then(IntrinsicType::from_str)
                .is_some_and(|it| it.0 == tag) =>
        {
            true
        }

        (TypeKind::Named(dn, origin_d), TypeKind::Named(in_, origin_i))
        | (TypeKind::Named(dn, origin_d), TypeKind::Generic(in_, _, origin_i))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Named(in_, origin_i))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Generic(in_, _, origin_i)) => {
            let interner = ctx_interner(bind);
            match interner {
                Some(interner) => compatible_named(
                    interner.resolve(dn),
                    origin_d.map(|o| interner.resolve(o)),
                    interner.resolve(in_),
                    origin_i.map(|o| interner.resolve(o)),
                    bind,
                    cache,
                    in_progress,
                    table,
                ),
                None => dn == in_,
            }
        }
        (TypeKind::Named(dn, origin_d), TypeKind::Fn(ft))
        | (TypeKind::Generic(dn, _, origin_d), TypeKind::Fn(ft)) => {
            let (Some(bind), Some(interner)) = (bind, ctx_interner(bind)) else {
                return true;
            };
            let dn_s = interner.resolve(dn);
            let origin_d_s = origin_d.map(|o| interner.resolve(o));
            let _ = ft;
            if let Some(members) = named_members(bind, dn_s, origin_d_s) {
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
            let dn_s = ctx_interner(bind).map(|i| i.resolve(dn));
            let arg_ids = table.get_list(args).to_vec();
            if dn_s != Some(IntrinsicType::Map.as_str()) || !(arg_ids.len() == 1 || arg_ids.len() == 2) {
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
            let interner = ctx_interner(bind);
            let dn_s = interner.map(|i| i.resolve(dn));
            if dn_s == Some(IntrinsicType::Map.as_str()) {
                true
            } else if let (Some(bind), Some(interner)) = (bind, interner) {
                let dn_s = interner.resolve(dn);
                let origin_d_s = origin_d.map(|o| interner.resolve(o));
                if let Some(decl_members) = named_members(bind, dn_s, origin_d_s) {
                    class_members_match_object(
                        &decl_members,
                        table.get_object_members(inf_fields),
                        bind,
                        cache,
                        in_progress,
                        table,
                    )
                } else {
                    !is_known_named(bind, dn_s)
                }
            } else {
                true
            }
        }
        (TypeKind::Object(decl_fields), TypeKind::Generic(in_, args, _)) => {
            let in_s = ctx_interner(bind).map(|i| i.resolve(in_));
            let arg_ids = table.get_list(args).to_vec();
            if in_s != Some(IntrinsicType::Map.as_str()) || !(arg_ids.len() == 1 || arg_ids.len() == 2) {
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
            let interner = ctx_interner(bind);
            let in_s = interner.map(|i| i.resolve(in_));
            if in_s == Some(IntrinsicType::Map.as_str()) {
                true
            } else if let (Some(bind), Some(interner)) = (bind, interner) {
                let in_s = interner.resolve(in_);
                let origin_i_s = origin_i.map(|o| interner.resolve(o));
                if let Some(inf_members) = named_members(bind, in_s, origin_i_s) {
                    crate::checker::compat::helpers::object_matches_class_members(
                        table.get_object_members(decl_fields),
                        &inf_members,
                        bind,
                        cache,
                        in_progress,
                        table,
                    )
                } else {
                    !is_known_named(bind, in_s)
                }
            } else {
                true
            }
        }
        (TypeKind::Named(dn, _), _) => {
            if ctx_interner(bind).map(|i| i.resolve(dn)) == Some(IntrinsicType::Map.as_str()) {
                true
            } else {
                named_fallback(declared, inferred, dn, None, bind, cache, in_progress, table, true)
            }
        }
        (_, TypeKind::Named(in_, _)) => {
            if ctx_interner(bind).map(|i| i.resolve(in_)) == Some(IntrinsicType::Map.as_str()) {
                true
            } else {
                named_fallback(declared, inferred, in_, None, bind, cache, in_progress, table, false)
            }
        }
        (TypeKind::Generic(name, args, _origin), _) => {
            let name_s = ctx_interner(bind).map(|i| i.resolve(name));
            let list = table.get_list(args);
            if name_s == Some(IntrinsicType::Task.as_str()) && list.len() == 1 {
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
    let Some(interner) = ctx_interner(Some(bind)) else {
        return false;
    };
    let name_s = interner.resolve(name);
    let origin_s = origin.map(|o| interner.resolve(o));
    let Some(expanded) = bind.resolve_type_alias(name_s, origin_s) else {
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

fn m_ty(m: &crate::types::ClassMemberInfo) -> Type {
    m.ty
}
