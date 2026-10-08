use super::compat_lookup::types_compatible_with_fn_signature;
use super::core::types_compatible_impl;
use super::scalar::t;
use crate::binder::BindView;
use crate::types::{CheckerTyTable, ObjectMembersId, ObjectTypeMember, Type};
use rustc_hash::{FxHashMap, FxHashSet};

pub(super) fn object_arm(
    decl_fields: ObjectMembersId,
    inf_fields: ObjectMembersId,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
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
                    ObjectTypeMember::Property { .. }
                    | ObjectTypeMember::Method { .. }
                    | ObjectTypeMember::Index { .. }
                    | ObjectTypeMember::Callable { .. } => None,
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
                    ObjectTypeMember::Property { .. }
                    | ObjectTypeMember::Method { .. }
                    | ObjectTypeMember::Index { .. }
                    | ObjectTypeMember::Callable { .. } => None,
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
                    ObjectTypeMember::Property { .. }
                    | ObjectTypeMember::Method { .. }
                    | ObjectTypeMember::Callable { .. } => false,
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
                    ObjectTypeMember::Index { .. } | ObjectTypeMember::Callable { .. } => true,
                });
                if !explicit_members_compatible {
                    ok = false;
                    break 'outer;
                }
            }
            ObjectTypeMember::Callable { .. } => {}
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
                        ObjectTypeMember::Index { .. } | ObjectTypeMember::Callable { .. } => false,
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
