use crate::types::{
    CheckerTyId, CheckerTyTable, FunctionType, ObjectTypeMember, Type, TypeContext,
};
use std::sync::Arc;
use varn_core::TypeKind;

pub(super) fn resolve_keyof(
    ty: Type,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    if let TypeKind::Object(mid) = table.get(ty.0) {
        let mut key_types: Vec<CheckerTyId> = vec![];
        for member in table.get_object_members(mid).to_vec() {
            if let ObjectTypeMember::Index { key_ty, .. } = member {
                if !key_types.contains(&key_ty) {
                    key_types.push(key_ty);
                }
            }
        }
        if !key_types.is_empty() {
            return if key_types.len() == 1 {
                Type(key_types[0], false)
            } else {
                let types: Vec<Type> = key_types.into_iter().map(|id| Type(id, false)).collect();
                Type::union(types, table)
            };
        }
    }

    let keys = collect_type_keys(&ty, ctx, table);
    if keys.is_empty() {
        Type::Never
    } else {
        Type::Str
    }
}

pub(super) fn collect_type_keys(
    ty: &Type,
    ctx: Option<&dyn TypeContext>,
    table: &CheckerTyTable,
) -> Vec<Arc<str>> {
    match table.get(ty.0) {
        TypeKind::Object(mid) => table
            .get_object_members(mid)
            .iter()
            .filter_map(|m| match m {
                ObjectTypeMember::Property { name, .. } => Some(name.clone()),
                ObjectTypeMember::Method { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect(),
        TypeKind::Named(name, origin) => {
            let name_str = ctx.and_then(|c| c.atom_text(name));
            let origin_str = origin.and_then(|o| ctx.and_then(|c| c.atom_text(o)));
            name_str
                .and_then(|name_str| {
                    ctx.and_then(|c| {
                        c.get_interface_members(&name_str, origin_str.as_deref())
                            .or_else(|| c.get_class_members(&name_str, origin_str.as_deref()))
                    })
                })
                .map(|members| members.iter().map(|m| m.name.clone()).collect())
                .unwrap_or_default()
        }
        TypeKind::Intersection(list) => {
            let mut all_keys: Vec<Arc<str>> = vec![];
            for part in table.get_list(list).to_vec() {
                for key in collect_type_keys(&Type(part, false), ctx, table) {
                    if !all_keys.contains(&key) {
                        all_keys.push(key);
                    }
                }
            }
            all_keys
        }
        TypeKind::Union(list) => {
            let parts = table.get_list(list).to_vec();
            if parts.is_empty() {
                return vec![];
            }
            let first = collect_type_keys(&Type(parts[0], false), ctx, table);
            first
                .into_iter()
                .filter(|k| {
                    parts[1..]
                        .iter()
                        .all(|p| collect_type_keys(&Type(*p, false), ctx, table).contains(k))
                })
                .collect()
        }
        _ => vec![],
    }
}

pub(super) fn resolve_indexed_access(
    obj: Type,
    index: Type,
    ctx: Option<&dyn TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let key_name_atom = match table.get(index.0) {
        TypeKind::Named(name, _) => Some(name),
        _ => None,
    };
    let key_name = key_name_atom.and_then(|a| ctx.and_then(|c| c.interner()).map(|i| i.resolve(a)));

    if let TypeKind::Object(mid) = table.get(obj.0) {
        let members = table.get_object_members(mid).to_vec();
        if let Some(key_name) = key_name {
            for m in &members {
                match m {
                    ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == key_name => {
                        return Type(*ty, false);
                    }
                    ObjectTypeMember::Method {
                        name,
                        params,
                        return_type,
                        is_arrow,
                        ..
                    } if name.as_ref() == key_name => {
                        return Type::fn_(
                            FunctionType {
                                params: params.clone(),
                                return_type: *return_type,
                                is_arrow: *is_arrow,
                                type_params: vec![],
                            },
                            table,
                        );
                    }
                    _ => {}
                }
            }
        }

        match table.get(index.0).clone() {
            TypeKind::Union(list) => {
                let ids = table.get_list(list).to_vec();
                let resolved: Vec<Type> = ids
                    .into_iter()
                    .map(|m| resolve_indexed_access(obj, Type(m, false), ctx, table))
                    .filter(|m| !m.is_dynamic())
                    .collect();
                return match resolved.len() {
                    0 => Type::Dynamic,
                    1 => resolved.into_iter().next().unwrap(),
                    _ => Type::union(resolved, table),
                };
            }
            _ => {
                let value_from_index = members.iter().find_map(|m| match m {
                    ObjectTypeMember::Index {
                        key_ty, value_ty, ..
                    } if crate::checker::compat::types_compatible(
                        &Type(*key_ty, false),
                        &index,
                        None,
                        table,
                    ) =>
                    {
                        Some(*value_ty)
                    }
                    _ => None,
                });
                if let Some(v) = value_from_index {
                    return Type(v, false);
                }
            }
        }
    }

    if let TypeKind::Named(name, origin) = table.get(obj.0) {
        let name = name;
        let origin = origin;
        if let Some(key_name) = key_name {
            let name_str = ctx.and_then(|c| c.atom_text(name));
            let origin_str = origin.and_then(|o| ctx.and_then(|c| c.atom_text(o)));
            if let Some(name_str) = name_str {
                if let Some(members) = ctx.and_then(|c| {
                    c.get_interface_members(&name_str, origin_str.as_deref())
                        .or_else(|| c.get_class_members(&name_str, origin_str.as_deref()))
                }) {
                    for m in members {
                        if m.name.as_ref() == key_name {
                            return m.ty;
                        }
                    }
                }
            }
        }
    }

    match table.get(index.0).clone() {
        TypeKind::Union(list) => {
            let ids = table.get_list(list).to_vec();
            let types: Vec<Type> = ids
                .into_iter()
                .map(|m| resolve_indexed_access(obj, Type(m, false), ctx, table))
                .collect();
            match types.len() {
                0 => Type::Dynamic,
                1 => types.into_iter().next().unwrap(),
                _ => Type::union(types, table),
            }
        }
        _ => {
            use varn_core::TypeTag;
            if matches!(table.get(obj.0), TypeKind::Intrinsic(TypeTag::Str))
                && matches!(table.get(index.0), TypeKind::Intrinsic(TypeTag::Int))
            {
                return Type::Str;
            }
            Type::Dynamic
        }
    }
}
