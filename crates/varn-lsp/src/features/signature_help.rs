use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, SignatureHelp, SignatureInformation,
};
use varn_core::TokenKind;

use crate::document::DocumentState;

pub fn build_signature_help(state: &DocumentState, line: u32, col: u32) -> Option<SignatureHelp> {
    let before: Vec<_> = state
        .tokens
        .iter()
        .filter(|t| t.line < line || (t.line == line && t.col < col))
        .collect();

    let mut depth: i32 = 0;
    let mut active_param: u32 = 0;
    let mut call_paren_idx: Option<usize> = None;

    for (idx, tok) in before.iter().enumerate().rev() {
        match tok.kind {
            TokenKind::RParen | TokenKind::RBracket => depth += 1,

            TokenKind::LBracket => {
                if depth > 0 {
                    depth -= 1;
                }
            }

            TokenKind::LParen => {
                if depth > 0 {
                    depth -= 1;
                } else {
                    call_paren_idx = Some(idx);
                    break;
                }
            }

            TokenKind::Comma if depth == 0 => active_param += 1,
            TokenKind::Semicolon if depth == 0 => break,

            _ => {}
        }
    }

    let call_idx = call_paren_idx?;
    let fn_tok = call_idx.checked_sub(1).and_then(|i| before.get(i))?;

    // 1. Direct Checker type info or lexical scope resolution on callee token
    if let Some(resolved) =
        resolve_callee_signature(state, fn_tok.line, fn_tok.col, &fn_tok.lexeme, active_param)
    {
        return Some(resolved);
    }

    // 2. Member call chain resolution (e.g. obj.method(arg))
    if let Some(chain) = state.resolve_chain_at(fn_tok.line, fn_tok.col) {
        use crate::document::ChainResult;
        let (params_str, ret_str) = match chain {
            ChainResult::Symbol(sym) => match state.db.fn_shape(sym.ty()) {
                Some(ft) => (format_params(state, &ft), state.db.id_text(ft.return_type)),
                None => (sym.params_str(), sym.type_str()),
            },
            // Derived from the member's type at render time; the record this
            // replaced carried both halves pre-flattened into `String`s.
            ChainResult::Member { member, .. } => match state.db.fn_shape(&member.ty) {
                Some(ft) => (format_params(state, &ft), state.db.id_text(ft.return_type)),
                None => (String::new(), state.ty_text(&member.ty)),
            },
        };
        return build_signature_response(&fn_tok.lexeme, &params_str, &ret_str, active_param);
    }

    None
}

fn resolve_callee_signature(
    state: &DocumentState,
    line: u32,
    col: u32,
    name: &str,
    active_param: u32,
) -> Option<SignatureHelp> {
    let tok = state
        .tokens
        .iter()
        .find(|t| t.line == line && t.col <= col && col < t.col + t.length)?;

    // 0. Check direct call_resolutions
    if let Some(call_res) = state.db.call_resolutions.get(&tok.offset) {
        let params_str = call_res
            .params
            .iter()
            .map(|p| {
                let n = p.name.as_deref().unwrap_or("arg");
                let opt = if p.optional { "?" } else { "" };
                format!("{}{}: {}", n, opt, state.ty_text(&p.ty))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let ret_str = state.ty_text(&call_res.return_ty);
        let name_str = call_res.callee_name.as_deref().unwrap_or(name);
        return build_signature_response(name_str, &params_str, &ret_str, active_param);
    }

    // 0b. Check direct member_resolutions
    if let Some(mem_res) = state.db.member_resolutions.get(&tok.offset) {
        if let Some(ft) = state.db.fn_shape(&mem_res.member_ty) {
            let params_str = format_params(state, &ft);
            let ret_str = state.db.id_text(ft.return_type);
            return build_signature_response(
                &mem_res.member_name,
                &params_str,
                &ret_str,
                active_param,
            );
        }
    }

    // 1. Check direct expr_types
    if let Some(info) = state.db.expr_types.get(&tok.offset) {
        if let Some(ft) = state.db.callable_shape(&info.ty) {
            let params_str = format_params(state, &ft);
            let ret_str = state.db.id_text(ft.return_type);
            return build_signature_response(name, &params_str, &ret_str, active_param);
        }
    }

    // 2. Check lexical scope resolution
    if let Some((_, ty)) = state.db.resolve_at(name, tok.offset) {
        if let Some(ft) = state.db.fn_shape(&ty) {
            let params_str = format_params(state, &ft);
            let ret_str = state.db.id_text(ft.return_type);
            return build_signature_response(name, &params_str, &ret_str, active_param);
        }
    }

    None
}

fn format_params(state: &DocumentState, ft: &varn_checker::types::FunctionType) -> String {
    ft.params
        .iter()
        .map(|p| {
            let n = p.name.as_deref().unwrap_or("arg");
            let opt = if p.optional { "?" } else { "" };
            format!("{}{}: {}", n, opt, state.db.id_text(p.ty))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn build_signature_response(
    fn_name: &str,
    params_str: &str,
    ret_str: &str,
    active_param: u32,
) -> Option<SignatureHelp> {
    let param_strs = split_params(params_str);
    let label = format!("{}({}): {}", fn_name, param_strs.join(", "), ret_str);

    let parameters: Vec<ParameterInformation> = param_strs
        .iter()
        .map(|p| ParameterInformation {
            label: ParameterLabel::Simple(p.clone()),
            documentation: None,
        })
        .collect();

    let active = if active_param < parameters.len() as u32 {
        Some(active_param)
    } else {
        None
    };

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: None,
            parameters: Some(parameters),
            active_parameter: active,
        }],
        active_signature: Some(0),
        active_parameter: active,
    })
}

fn split_params(params_str: &str) -> Vec<String> {
    if params_str.trim().is_empty() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();

    for ch in params_str.chars() {
        match ch {
            '<' | '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            '>' | ')' | ']' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    parts.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push(trimmed);
    }
    parts
}
