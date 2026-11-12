use super::core::types_compatible_impl;
use super::scalar::t;
use crate::bind::BindView;
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
                name,
                ty,
                optional,
                readonly,
            } => {
                let found = inf_fields.iter().find_map(|im| match im {
                    ObjectTypeMember::Property {
                        name: iname,
                        ty: ity,
                        readonly: source_readonly,
                        ..
                    } if iname == name => Some((*ity, *source_readonly)),
                    ObjectTypeMember::Property { .. }
                    | ObjectTypeMember::Method { .. }
                    | ObjectTypeMember::Callable { .. } => None,
                });
                match found {
                    Some((inf_ty, source_readonly)) => {
                        if source_readonly && !*readonly {
                            return false;
                        }
                        let fits = if *readonly {
                            types_compatible_impl(
                                &t(*ty),
                                &t(inf_ty),
                                bind,
                                cache,
                                in_progress,
                                table,
                            )
                        } else {
                            super::arrays::same_element(
                                &t(*ty),
                                &t(inf_ty),
                                bind,
                                cache,
                                in_progress,
                                table,
                            )
                        };
                        if !fits {
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
            ObjectTypeMember::Callable { .. } => {}
        }
    }

    if ok {
        for im in &inf_fields {
            if let ObjectTypeMember::Property { name: iname, .. } = im {
                let exists_in_decl = decl_fields.iter().any(|dm| match dm {
                    ObjectTypeMember::Property { name: dname, .. } => dname == iname,
                    ObjectTypeMember::Method { name: dname, .. } => dname == iname,
                    ObjectTypeMember::Callable { .. } => false,
                });
                if !exists_in_decl {
                    ok = false;
                    break;
                }
            }
        }
    }
    ok
}

pub(super) fn record_arm(
    decl_fields: ObjectMembersId,
    inf_fields: ObjectMembersId,
    bind: Option<&BindView>,
    cache: &mut FxHashMap<(Type, Type, usize), bool>,
    in_progress: &mut FxHashSet<(Type, Type, usize)>,
    table: &CheckerTyTable,
) -> bool {
    let decl = table.get_object_members(decl_fields);
    let inf = table.get_object_members(inf_fields);
    let shared_names_agree = inf.iter().all(|im| match im {
        ObjectTypeMember::Property { name, optional, .. } => decl.iter().any(|dm| {
            matches!(
                dm,
                ObjectTypeMember::Property { name: n, optional: o, .. }
                    if n == name && o == optional
            )
        }),
        ObjectTypeMember::Method { .. } | ObjectTypeMember::Callable { .. } => true,
    });
    let every_field_is_given = decl.iter().all(|dm| match dm {
        ObjectTypeMember::Property { name, .. } => inf
            .iter()
            .any(|im| matches!(im, ObjectTypeMember::Property { name: n, .. } if n == name)),
        ObjectTypeMember::Method { .. } | ObjectTypeMember::Callable { .. } => true,
    });
    shared_names_agree
        && every_field_is_given
        && object_arm(decl_fields, inf_fields, bind, cache, in_progress, table)
}
