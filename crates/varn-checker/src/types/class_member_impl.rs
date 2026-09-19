use super::*;

impl ClassMemberInfo {
    pub fn params_str(&self, table: &CheckerTyTable, interner: &varn_core::AtomInterner) -> String {
        match table.get(self.ty.0) {
            TypeKind::Fn(fid) => format_fn_params(&table.get_function(*fid).params.clone(), table, interner),
            _ => String::new(),
        }
    }

    pub fn return_type_str(&self, table: &CheckerTyTable, interner: &varn_core::AtomInterner) -> String {
        match table.get(self.ty.0) {
            TypeKind::Fn(fid) => {
                let ret = table.get_function(*fid).return_type;
                Type(ret, false).display(table, interner).to_string()
            }
            _ => self.ty.display(table, interner).to_string(),
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
            let ty_str = Type(p.ty, false).display(table, interner).to_string();
            match &p.name {
                Some(n) => format!("{rest}{n}{opt}: {ty_str}"),
                None => format!("{rest}{ty_str}"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
