use super::*;

impl ClassMemberInfo {
    pub fn as_object_member(&self, table: &CheckerTyTable) -> ObjectTypeMember {
        if let (ClassMemberKind::Method, TypeKind::Fn(fid)) = (self.kind, table.get(self.ty.0)) {
            let ft = table.get_function(fid);
            return ObjectTypeMember::Method {
                name: self.name.clone(),
                params: ft.params.clone(),
                return_type: ft.return_type,
                optional: self.is_optional,
                is_arrow: ft.is_arrow,
            };
        }
        ObjectTypeMember::Property {
            name: self.name.clone(),
            ty: self.ty.0,
            optional: self.is_optional,
            readonly: self.is_readonly,
        }
    }

    pub fn params_str(&self, table: &CheckerTyTable, interner: &varn_core::AtomInterner) -> String {
        match table.get(self.ty.0) {
            TypeKind::Fn(fid) => {
                format_fn_params(&table.get_function(fid).params.clone(), table, interner)
            }
            TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_) | TypeKind::This | TypeKind::Array(_) | TypeKind::Union(_) | TypeKind::Intersection(_) | TypeKind::Tuple(_) | TypeKind::Named(..) | TypeKind::Generic(..) | TypeKind::TemplateLiteral(_) | TypeKind::Object(_) | TypeKind::Typeof(_) | TypeKind::KeyOf(_) | TypeKind::IndexedAccess { .. } | TypeKind::Mapped { .. } | TypeKind::Conditional { .. } | TypeKind::Infer(_) | TypeKind::EnumVariant { .. } | TypeKind::TypePredicate { .. } => String::new(),
        }
    }

    pub fn return_type_str(
        &self,
        table: &CheckerTyTable,
        interner: &varn_core::AtomInterner,
    ) -> String {
        match table.get(self.ty.0) {
            TypeKind::Fn(fid) => {
                let ret = table.get_function(fid).return_type;
                Type::resolved(ret).display(table, interner).to_string()
            }
            TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_) | TypeKind::This | TypeKind::Array(_) | TypeKind::Union(_) | TypeKind::Intersection(_) | TypeKind::Tuple(_) | TypeKind::Named(..) | TypeKind::Generic(..) | TypeKind::TemplateLiteral(_) | TypeKind::Object(_) | TypeKind::Typeof(_) | TypeKind::KeyOf(_) | TypeKind::IndexedAccess { .. } | TypeKind::Mapped { .. } | TypeKind::Conditional { .. } | TypeKind::Infer(_) | TypeKind::EnumVariant { .. } | TypeKind::TypePredicate { .. } => self.ty.display(table, interner).to_string(),
        }
    }
}

fn format_fn_params(
    params: &[FunctionParam],
    table: &CheckerTyTable,
    interner: &varn_core::AtomInterner,
) -> String {
    params
        .iter()
        .map(|p| {
            let rest = if p.is_rest { "..." } else { "" };
            let opt = if p.optional { "?" } else { "" };
            let ty_str = Type::resolved(p.ty).display(table, interner).to_string();
            match &p.name {
                Some(n) => format!("{rest}{n}{opt}: {ty_str}"),
                None => format!("{rest}{ty_str}"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
