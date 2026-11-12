use super::{CheckerTyTable, ObjectTypeMember, Type};
use std::sync::Arc;
use vn_core::{Atom, TypeKind};

impl Type {
    pub fn extension_key(
        &self,
        table: &CheckerTyTable,
        name_of: &dyn Fn(Atom) -> Option<Arc<str>>,
    ) -> Option<Arc<str>> {
        match table.get(self.0) {
            TypeKind::Named(name, _) | TypeKind::Generic(name, _, _) => name_of(name),
            TypeKind::Primitive(primitive) => Some(Arc::from(primitive.name())),
            TypeKind::Literal(literal) => Some(Arc::from(literal.base().name())),
            TypeKind::Array(inner) => {
                let inner = Type::resolved(inner).extension_key(table, name_of)?;
                Some(Arc::from(format!("{inner}[]")))
            }
            TypeKind::Tuple(list) => {
                let parts: Option<Vec<Arc<str>>> = table
                    .get_list(list)
                    .iter()
                    .map(|id| Type::resolved(*id).extension_key(table, name_of))
                    .collect();
                Some(Arc::from(format!("({})", parts?.join(","))))
            }
            TypeKind::Record(fields) => {
                let mut parts: Vec<String> = Vec::new();
                for member in table.get_object_members(fields) {
                    match member {
                        ObjectTypeMember::Property {
                            name, ty, optional, ..
                        } => {
                            let value = Type::resolved(*ty).extension_key(table, name_of)?;
                            let mark = if *optional { "?" } else { "" };
                            parts.push(format!("{name}{mark}:{value}"));
                        }
                        ObjectTypeMember::Method { .. } | ObjectTypeMember::Callable { .. } => {
                            return None
                        }
                    }
                }
                parts.sort();
                Some(Arc::from(format!("{{{}}}", parts.join(";"))))
            }
            TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        }
    }
}
