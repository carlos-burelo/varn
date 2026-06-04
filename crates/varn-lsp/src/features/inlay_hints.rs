use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position};
use varn_checker::SymbolKind;
use varn_core::TypeKind;

use crate::document::DocumentState;

pub fn build_inlay_hints(state: &DocumentState) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    for s in &state.symbols {
        if s.line == u32::MAX {
            continue;
        }
        if s.is_from_stdlib {
            continue;
        }

        match s.kind {
            SymbolKind::Const | SymbolKind::Let | SymbolKind::Var
                if !s.has_explicit_type && !s.type_str.is_empty() =>
            {
                let hint_col = s.col + s.name.len() as u32;
                hints.push(InlayHint {
                    position: Position {
                        line: s.line,
                        character: hint_col,
                    },
                    label: InlayHintLabel::String(format!(": {}", s.type_str)),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(false),
                    padding_right: Some(true),
                    data: None,
                });
            }

            SymbolKind::Function | SymbolKind::Method => {
                if let Some(hint) = fn_return_hint(state, s) {
                    hints.push(hint);
                }
            }

            _ => {}
        }
    }

    hints
}

fn fn_return_hint(state: &DocumentState, sym: &crate::document::SymbolRecord) -> Option<InlayHint> {
    if sym.has_explicit_type {
        return None;
    }

    if line_has_explicit_return_annotation(state, sym.line) {
        return None;
    }

    let ret_ty = match &sym.ty.0 {
        TypeKind::Fn(ft) => ft.return_type.as_ref(),
        _ => return None,
    };

    match &ret_ty.0 {
        TypeKind::Intrinsic(varn_core::TypeTag::Void | varn_core::TypeTag::Dynamic) => return None,
        _ => {}
    }
    let ret_str = ret_ty.to_string();
    if ret_str.is_empty() || ret_str == "unknown" || ret_str == "void" {
        return None;
    }

    let rparen_col = find_rparen_col_on_line(state, sym.line, sym.col)?;

    Some(InlayHint {
        position: Position {
            line: sym.line,
            character: rparen_col + 1,
        },
        label: InlayHintLabel::String(format!(": {ret_str}")),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(true),
        data: None,
    })
}

fn line_has_explicit_return_annotation(state: &DocumentState, line: u32) -> bool {
    let line_tokens: Vec<_> = state.tokens.iter().filter(|t| t.line == line).collect();
    let mut depth = 0i32;
    let mut after_last_rparen = false;
    let mut found_colon_after_rparen = false;
    for tok in &line_tokens {
        match tok.kind {
            varn_core::TokenKind::LParen => {
                depth += 1;
                after_last_rparen = false;
            }
            varn_core::TokenKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    after_last_rparen = true;
                    found_colon_after_rparen = false;
                }
            }
            varn_core::TokenKind::Colon if after_last_rparen => {
                found_colon_after_rparen = true;
            }
            varn_core::TokenKind::LBrace if found_colon_after_rparen => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn find_rparen_col_on_line(state: &DocumentState, line: u32, fn_col: u32) -> Option<u32> {
    let mut depth = 0i32;
    let mut rparen_col = None;
    for tok in state
        .tokens
        .iter()
        .filter(|t| t.line == line && t.col >= fn_col)
    {
        match tok.kind {
            varn_core::TokenKind::LParen => depth += 1,
            varn_core::TokenKind::RParen => {
                depth -= 1;
                if depth == 0 {
                    rparen_col = Some(tok.col + tok.length.saturating_sub(1));
                    break;
                }
            }
            _ => {}
        }
    }
    rparen_col
}
