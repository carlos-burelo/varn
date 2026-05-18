use std::collections::{HashMap, HashSet};

use crate::document::{ParamScope, TokenRecord};
use varn_core::TokenKind;

pub fn collect_param_scopes(tokens: &[TokenRecord]) -> Vec<ParamScope> {
    let mut scopes = Vec::new();
    let n = tokens.len();
    let mut i = 0;
    while i < n {
        if tokens[i].kind == TokenKind::LParen {
            let start_idx = i;
            let mut depth = 1;
            i += 1;
            while i < n && depth > 0 {
                match tokens[i].kind {
                    TokenKind::LParen => depth += 1,
                    TokenKind::RParen => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            if i < n {
                let end_paren_idx = i - 1;
                let mut found_body = false;
                let mut body_start_line = 0;
                let mut next = i;
                if next < n && tokens[next].kind == TokenKind::Arrow {
                    next += 1;
                }
                if next < n && tokens[next].kind == TokenKind::Colon {
                    next += 1;
                    let mut angle_depth = 0i32;
                    while next < n {
                        match tokens[next].kind {
                            TokenKind::LAngle => {
                                angle_depth += 1;
                                next += 1;
                            }
                            TokenKind::RAngle if angle_depth > 0 => {
                                angle_depth -= 1;
                                next += 1;
                            }
                            TokenKind::LBrace | TokenKind::Semicolon => break,
                            _ => {
                                next += 1;
                            }
                        }
                    }
                }
                if next < n && tokens[next].kind == TokenKind::LBrace {
                    found_body = true;
                    body_start_line = tokens[next].line;
                }

                if found_body {
                    let params = parse_params_from_tokens(&tokens[start_idx + 1..end_paren_idx]);
                    let mut b_depth = 1;
                    let mut j = next + 1;
                    while j < tokens.len() && b_depth > 0 {
                        match tokens[j].kind {
                            TokenKind::LBrace => b_depth += 1,
                            TokenKind::RBrace => b_depth -= 1,
                            _ => {}
                        }
                        j += 1;
                    }
                    let body_end_line = if j > 0 {
                        tokens[j - 1].line
                    } else {
                        body_start_line
                    };
                    scopes.push(ParamScope {
                        body_start_line,
                        body_end_line,
                        params,
                    });
                }
            }
        }
        i += 1;
    }
    scopes
}

fn parse_params_from_tokens(tokens: &[TokenRecord]) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].kind != TokenKind::Identifier {
            i += 1;
            continue;
        }
        let name = tokens[i].lexeme.clone();
        i += 1;
        let type_str = if i < tokens.len() && tokens[i].kind == TokenKind::Colon {
            i += 1;
            let mut parts: Vec<(u32, u32, &str)> = Vec::new();
            let mut depth = 0i32;
            while i < tokens.len() {
                match tokens[i].kind {
                    TokenKind::LParen | TokenKind::LAngle => depth += 1,
                    TokenKind::RParen if depth == 0 => break,
                    TokenKind::RParen => depth -= 1,
                    TokenKind::RAngle if depth > 0 => depth -= 1,
                    TokenKind::Comma if depth == 0 => break,
                    TokenKind::Eq if depth == 0 => break,
                    _ => {}
                }
                parts.push((tokens[i].line, tokens[i].col, &tokens[i].lexeme));
                i += 1;
            }
            reconstruct_type(&parts)
        } else {
            String::new()
        };

        params.push((name, type_str));
        while i < tokens.len() && tokens[i].kind != TokenKind::Comma {
            i += 1;
        }
        if i < tokens.len() {
            i += 1;
        }
    }
    params
}

fn reconstruct_type(parts: &[(u32, u32, &str)]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut prev_line = parts[0].0;
    let mut prev_end = parts[0].1;
    for (ln, col, lex) in parts {
        if !out.is_empty() && (*ln > prev_line || *col > prev_end) {
            out.push(' ');
        }
        out.push_str(lex);
        prev_line = *ln;
        prev_end = col + lex.len() as u32;
    }
    out
}

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
