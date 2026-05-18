use crate::document::TokenRecord;
use varn_core::TokenKind;

pub fn format_type_params(ty: &varn_checker::Type) -> String {
    use varn_checker::types::FunctionType;
    use varn_core::TypeKind;
    if let TypeKind::Fn(FunctionType { params, .. }) = &ty.0 {
        let mut out = String::new();
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            if let Some(name) = &p.name {
                out.push_str(name);
            } else {
                out.push_str(&format!("arg{i}"));
            }
            if p.optional {
                out.push('?');
            }
            out.push_str(": ");
            out.push_str(&format!("{}", p.ty));
        }
        out
    } else {
        String::new()
    }
}

pub fn extract_enum_init_value(tokens: &[TokenRecord], line: u32) -> String {
    let mut found_eq = false;
    for tok in tokens.iter().filter(|t| t.line == line) {
        if found_eq {
            return tok.lexeme.clone();
        }
        if tok.kind == TokenKind::Eq {
            found_eq = true;
        }
    }
    String::new()
}
