use super::*;

impl ClassMemberInfo {
    pub fn params_str(&self) -> String {
        match &self.ty.0 {
            TypeKind::Fn(ft) => format_fn_params(&ft.params),
            _ => String::new(),
        }
    }

    pub fn return_type_str(&self) -> String {
        match &self.ty.0 {
            TypeKind::Fn(ft) => ft.return_type.to_string(),
            _ => self.ty.to_string(),
        }
    }
}

fn format_fn_params(params: &[FunctionParam]) -> String {
    params
        .iter()
        .map(|p| {
            let rest = if p.is_rest { "..." } else { "" };
            let opt = if p.optional { "?" } else { "" };
            match &p.name {
                Some(n) => format!("{rest}{n}{opt}: {}", p.ty),
                None => format!("{rest}{}", p.ty),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
