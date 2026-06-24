use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, SignatureHelp, SignatureInformation,
};
use varn_core::{TokenKind, TypeKind};

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

    if let Some(resolved) =
        resolve_callee_signature(state, fn_tok.line, fn_tok.col, &fn_tok.lexeme, active_param)
    {
        return Some(resolved);
    }

    if let Some(chain) = state.resolve_chain_at(fn_tok.line, fn_tok.col) {
        use crate::document::ChainResult;
        let (params_str, ret_str) = match chain {
            ChainResult::Symbol(sym) => split_arrow_type(&sym.type_str)?,
            ChainResult::Member { member, .. } => {
                (member.params_str.clone(), member.type_str.clone())
            }
            ChainResult::DynamicMember { member, .. } => {
                (member.params_str.clone(), member.type_str.clone())
            }
        };
        return build_signature_response(&fn_tok.lexeme, &params_str, &ret_str, active_param);
    }

    if fn_tok.kind != TokenKind::Identifier {
        return None;
    }
    let fn_name = fn_tok.lexeme.as_str();
    let sym = state.symbols.iter().find(|s| s.name == fn_name)?;
    if sym.type_str.is_empty() {
        return None;
    }

    let (params_str, ret_str) = split_arrow_type(&sym.type_str)?;
    build_signature_response(fn_name, &params_str, &ret_str, active_param)
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

    let info = state.db.expr_types.get(&tok.offset)?;
    if let TypeKind::Fn(ft) = &info.ty.0 {
        let params_str = ft
            .params
            .iter()
            .map(|p| {
                let n = p.name.as_deref().unwrap_or("arg");
                let opt = if p.optional { "?" } else { "" };
                format!("{}{}: {}", n, opt, p.ty)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let ret_str = ft.return_type.to_string();
        return build_signature_response(name, &params_str, &ret_str, active_param);
    }

    None
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

fn split_arrow_type(ty: &str) -> Option<(String, String)> {
    let ty = ty.trim();
    if !ty.starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    let mut close = None;
    for (i, ch) in ty.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let params = ty[1..close].to_owned();
    let after = ty[close + 1..].trim();
    let ret = after.strip_prefix("=>")?.trim().to_owned();
    Some((params, ret))
}

fn split_params(params: &str) -> Vec<String> {
    if params.trim().is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut depth: u32 = 0;
    let mut current = String::new();
    for ch in params.chars() {
        match ch {
            '<' | '(' => {
                depth += 1;
                current.push(ch);
            }
            '>' | ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                result.push(current.trim().to_owned());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        result.push(current.trim().to_owned());
    }
    result
}
