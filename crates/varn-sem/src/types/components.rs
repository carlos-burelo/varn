use super::{CheckerTyTable, ObjectTypeMember, Type};
use varn_core::TypeKind;

impl Type {
    pub fn components(self, table: &CheckerTyTable) -> Vec<Type> {
        let mut ids = Vec::new();
        match table.get(self.0) {
            TypeKind::Array(inner) | TypeKind::KeyOf(inner) => ids.push(inner),
            TypeKind::Union(list)
            | TypeKind::Intersection(list)
            | TypeKind::Tuple(list)
            | TypeKind::TemplateLiteral(list)
            | TypeKind::Generic(_, list, _) => ids.extend_from_slice(table.get_list(list)),
            TypeKind::Fn(id) => {
                let f = table.get_function(id);
                ids.extend(f.params.iter().map(|p| p.ty));
                ids.push(f.return_type);
            }
            TypeKind::Object(id) => {
                for member in table.get_object_members(id) {
                    match member {
                        ObjectTypeMember::Property { ty, .. } => ids.push(*ty),
                        ObjectTypeMember::Method {
                            params,
                            return_type,
                            ..
                        }
                        | ObjectTypeMember::Callable {
                            params,
                            return_type,
                            ..
                        } => {
                            ids.extend(params.iter().map(|p| p.ty));
                            ids.push(*return_type);
                        }
                        ObjectTypeMember::Index {
                            key_ty, value_ty, ..
                        } => {
                            ids.extend([*key_ty, *value_ty]);
                        }
                    }
                }
            }
            TypeKind::IndexedAccess { object, index } => ids.extend([object, index]),
            TypeKind::Mapped { source, value, .. } => ids.extend([source, value]),
            TypeKind::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                ids.extend([check, extends, true_type, false_type]);
            }
            TypeKind::EnumVariant {
                type_args,
                payload_ty,
                ..
            } => {
                ids.extend_from_slice(table.get_list(type_args));
                ids.push(payload_ty);
            }
            TypeKind::TypePredicate { target_type, .. } => ids.push(target_type),
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Named(..)
            | TypeKind::Typeof(_)
            | TypeKind::Infer(_) => {}
        }
        ids.into_iter().map(Type::resolved).collect()
    }
}
