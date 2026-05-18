use crate::types::{FunctionType, ObjectTypeMember, Type, TypeContext};
use std::rc::Rc;
use varn_core::TypeKind;

pub(super) fn resolve_keyof(ty: Type, ctx: Option<&dyn TypeContext>) -> Type {
    if let TypeKind::Object(members) = &ty.0 {
        let mut key_types: Vec<Type> = vec![];
        for member in members {
            if let ObjectTypeMember::Index { key_ty, .. } = member {
                if !key_types.iter().any(|k| k == key_ty.as_ref()) {
                    key_types.push((**key_ty).clone());
                }
            }
        }
        if !key_types.is_empty() {
            return if key_types.len() == 1 {
                key_types.into_iter().next().unwrap()
            } else {
                Type::union(key_types)
            };
        }
    }

    let keys = collect_type_keys(&ty, ctx);
    match keys.len() {
        0 => Type::Never,
        1 => Type::literal_str(keys.into_iter().next().unwrap()),
        _ => Type::union(keys.into_iter().map(Type::literal_str).collect()),
    }
}

fn collect_type_keys(ty: &Type, ctx: Option<&dyn TypeContext>) -> Vec<Rc<str>> {
    match &ty.0 {
        TypeKind::Object(members) => members
            .iter()
            .filter_map(|m| match m {
                ObjectTypeMember::Property { name, .. } => Some(name.clone()),
                ObjectTypeMember::Method { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect(),
        TypeKind::Named(name, _) => ctx
            .and_then(|c| {
                c.get_interface_members(name.as_ref(), None)
                    .or_else(|| c.get_class_members(name.as_ref(), None))
            })
            .map(|members| members.iter().map(|m| m.name.clone()).collect())
            .unwrap_or_default(),
        TypeKind::Intersection(parts) => {
            let mut all_keys: Vec<Rc<str>> = vec![];
            for part in parts {
                for key in collect_type_keys(part, ctx) {
                    if !all_keys.contains(&key) {
                        all_keys.push(key);
                    }
                }
            }
            all_keys
        }
        TypeKind::Union(parts) => {
            if parts.is_empty() {
                return vec![];
            }
            let first = collect_type_keys(&parts[0], ctx);
            first
                .into_iter()
                .filter(|k| {
                    parts[1..]
                        .iter()
                        .all(|p| collect_type_keys(p, ctx).contains(k))
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
) -> Type {
    if let TypeKind::Object(members) = &obj.0 {
        let value_from_index = |idx: &Type| {
            members.iter().find_map(|m| match m {
                ObjectTypeMember::Index {
                    key_ty, value_ty, ..
                } if crate::checker::compat::types_compatible(key_ty, idx, None) => {
                    Some((**value_ty).clone())
                }
                _ => None,
            })
        };

        match &index.0 {
            TypeKind::LiteralStr(key) => {
                if let Some(prop) = lookup_property_type(&obj, key.as_ref(), ctx) {
                    return prop;
                }
                if let Some(v) = value_from_index(&Type::Str) {
                    return v;
                }
            }
            TypeKind::LiteralInt(_) => {
                if let Some(v) = value_from_index(&Type::Int) {
                    return v;
                }
            }
            TypeKind::Union(members) => {
                let resolved: Vec<Type> = members
                    .iter()
                    .map(|m| resolve_indexed_access(obj.clone(), m.clone(), ctx))
                    .filter(|m| !m.is_dynamic())
                    .collect();
                return match resolved.len() {
                    0 => Type::Dynamic,
                    1 => resolved.into_iter().next().unwrap(),
                    _ => Type::union(resolved),
                };
            }
            _ => {
                if let Some(v) = value_from_index(&index) {
                    return v;
                }
            }
        }
    }

    match &index.0 {
        TypeKind::LiteralStr(key) => {
            lookup_property_type(&obj, key.as_ref(), ctx).unwrap_or(Type::Dynamic)
        }
        TypeKind::Union(members) => {
            let types: Vec<Type> = members
                .iter()
                .filter_map(|m| {
                    if let TypeKind::LiteralStr(key) = &m.0 {
                        lookup_property_type(&obj, key.as_ref(), ctx)
                    } else {
                        None
                    }
                })
                .collect();
            match types.len() {
                0 => Type::Dynamic,
                1 => types.into_iter().next().unwrap(),
                _ => Type::union(types),
            }
        }
        _ => {
            use varn_core::TypeTag;
            if matches!(obj.0, TypeKind::Intrinsic(TypeTag::Str))
                && matches!(index.0, TypeKind::Intrinsic(TypeTag::Int))
            {
                return Type::Str;
            }
            Type::Dynamic
        }
    }
}

fn lookup_property_type(ty: &Type, key: &str, ctx: Option<&dyn TypeContext>) -> Option<Type> {
    match &ty.0 {
        TypeKind::Object(members) => members.iter().find_map(|m| match m {
            ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == key => Some(ty.clone()),
            ObjectTypeMember::Method {
                name,
                params,
                return_type,
                is_arrow,
                ..
            } if name.as_ref() == key => Some(Type::fn_(FunctionType {
                params: params.clone(),
                return_type: return_type.clone(),
                is_arrow: *is_arrow,
                type_params: vec![],
            })),
            ObjectTypeMember::Index {
                key_ty, value_ty, ..
            } if crate::checker::compat::types_compatible(key_ty, &Type::Str, None) => {
                Some((**value_ty).clone())
            }
            _ => None,
        }),
        TypeKind::Named(name, _) => ctx.and_then(|c| {
            c.get_interface_members(name.as_ref(), None)
                .or_else(|| c.get_class_members(name.as_ref(), None))
                .and_then(|members| {
                    members
                        .iter()
                        .find(|m| m.name.as_ref() == key)
                        .map(|m| m.ty.clone())
                })
        }),
        _ => None,
    }
}
