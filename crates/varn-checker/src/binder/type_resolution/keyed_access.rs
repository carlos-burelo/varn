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
    if keys.is_empty() {
        Type::Never
    } else {
        Type::Str
    }
}

pub(super) fn collect_type_keys(ty: &Type, ctx: Option<&dyn TypeContext>) -> Vec<Rc<str>> {
    match &ty.0 {
        TypeKind::Object(members) => members
            .iter()
            .filter_map(|m| match m {
                ObjectTypeMember::Property { name, .. } => Some(name.clone()),
                ObjectTypeMember::Method { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect(),
        TypeKind::Named(name, origin) => ctx
            .and_then(|c| {
                c.get_interface_members(name.as_ref(), origin.as_deref())
                    .or_else(|| c.get_class_members(name.as_ref(), origin.as_deref()))
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
    let key_name_opt = match &index.0 {
        TypeKind::Named(name, _) => Some(name.as_ref()),
        _ => None,
    };

    if let TypeKind::Object(members) = &obj.0 {
        if let Some(key_name) = key_name_opt {
            for m in members {
                match m {
                    ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == key_name => {
                        return ty.clone();
                    }
                    ObjectTypeMember::Method {
                        name,
                        params,
                        return_type,
                        is_arrow,
                        ..
                    } if name.as_ref() == key_name => {
                        return Type::fn_(FunctionType {
                            params: params.clone(),
                            return_type: return_type.clone(),
                            is_arrow: *is_arrow,
                            type_params: vec![],
                        });
                    }
                    _ => {}
                }
            }
        }

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

    if let TypeKind::Named(name, origin) = &obj.0 {
        if let Some(key_name) = key_name_opt {
            if let Some(members) = ctx.and_then(|c| {
                c.get_interface_members(name.as_ref(), origin.as_deref())
                    .or_else(|| c.get_class_members(name.as_ref(), origin.as_deref()))
            }) {
                for m in members {
                    if m.name.as_ref() == key_name {
                        return m.ty;
                    }
                }
            }
        }
    }

    match &index.0 {
        TypeKind::Union(members) => {
            let types: Vec<Type> = members
                .iter()
                .filter_map(|m| resolve_indexed_access(obj.clone(), m.clone(), ctx).into())
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
