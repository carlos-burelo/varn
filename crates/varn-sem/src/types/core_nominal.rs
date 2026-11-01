use super::{CheckerTyTable, Type};
use varn_core::{BuiltinType, CoreSum, RuntimeErrorKind, TypeKind};

impl Type {
    pub fn builtin(b: BuiltinType, args: Vec<Type>, table: &mut CheckerTyTable) -> Self {
        let name = table.intern_name(b.name());
        let origin = Some(table.intern_name(b.core_module()));
        if args.is_empty() {
            Type::named_with_origin_atom(name, origin, table)
        } else {
            Type::generic_atom(name, args, origin, table)
        }
    }

    pub fn builtin_of(&self, table: &CheckerTyTable) -> Option<BuiltinType> {
        match table.get(self.0) {
            TypeKind::Array(_) => Some(BuiltinType::Array),
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => {
                BuiltinType::identify(name, origin)
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
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

    pub fn is_builtin(&self, b: BuiltinType, table: &CheckerTyTable) -> bool {
        self.builtin_of(table) == Some(b)
    }

    pub fn builtin_arg(&self, index: usize, table: &CheckerTyTable) -> Type {
        match table.get(self.0) {
            TypeKind::Array(inner) if index == 0 => Type::resolved(inner),
            TypeKind::Generic(_, args, _) => table
                .get_list(args)
                .get(index)
                .map_or(Type::Dynamic, |&id| Type::resolved(id)),
            TypeKind::Array(_)
            | TypeKind::Named(..)
            | TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => Type::Dynamic,
        }
    }

    pub fn core_sum(&self, table: &CheckerTyTable) -> Option<(CoreSum, Vec<Type>)> {
        let TypeKind::Generic(name, args, origin) = table.get(self.0) else {
            return None;
        };
        let sum = CoreSum::identify(name, origin)?;
        let args = table
            .get_list(args)
            .iter()
            .map(|&id| Type::resolved(id))
            .collect();
        Some((sum, args))
    }

    pub fn core_error(kind: RuntimeErrorKind, table: &mut CheckerTyTable) -> Self {
        let name = table.intern_name(kind.class_name());
        let origin = Some(table.intern_name(RuntimeErrorKind::MODULE));
        Type::named_with_origin_atom(name, origin, table)
    }
}
