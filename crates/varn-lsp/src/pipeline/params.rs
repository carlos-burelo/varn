use std::collections::{HashMap, HashSet};

use crate::document::TokenRecord;
use varn_core::TokenKind;

pub fn collect_type_params(
    tokens: &[TokenRecord],
) -> (HashMap<String, Vec<String>>, HashSet<String>) {
    let mut name_to_params: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_names: HashSet<String> = HashSet::new();
    let n = tokens.len();

    let mut i = 0;
    while i < n {
        if tokens[i].kind == TokenKind::LAngle && i >= 2 {
            let prev = &tokens[i - 1];
            let prev2 = &tokens[i - 2];
            if prev.kind == TokenKind::Identifier
                && matches!(
                    prev2.kind,
                    TokenKind::Class | TokenKind::Interface | TokenKind::Type | TokenKind::Function
                )
            {
                let sym_name = prev.lexeme.clone();
                let params = collect_type_param_names(tokens, i);
                for p in &params {
                    all_names.insert(p.clone());
                }
                name_to_params.insert(sym_name, params);
            }
        }
        i += 1;
    }

    (name_to_params, all_names)
}

fn collect_type_param_names(tokens: &[TokenRecord], langle_idx: usize) -> Vec<String> {
    let mut params = Vec::new();
    let mut depth = 1i32;
    let mut j = langle_idx + 1;

    while j < tokens.len() {
        match tokens[j].kind {
            TokenKind::LAngle => depth += 1,
            TokenKind::RAngle => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            TokenKind::Identifier if depth == 1 => {
                let prev_kind = tokens[j - 1].kind;
                if matches!(prev_kind, TokenKind::LAngle | TokenKind::Comma) {
                    params.push(tokens[j].lexeme.clone());
                }
            }
            _ => {}
        }
        j += 1;
    }

    params
}
