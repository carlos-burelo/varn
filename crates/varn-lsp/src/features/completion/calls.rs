use crate::document::DocumentState;
use rustc_hash::FxHashSet;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use varn_core::{TokenKind, TypeKind};

pub fn build_call_argument_completions(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<Vec<CompletionItem>> {
    let mut current_idx = state
        .tokens
        .iter()
        .position(|t| t.line > line || (t.line == line && t.col >= col))
        .unwrap_or(state.tokens.len());

    if current_idx > 0 {
        current_idx -= 1;
    }

    let mut balance = 0_i32;
    let mut lparen_idx = None;

    for i in (0..=current_idx).rev() {
        let t = &state.tokens[i];
        if t.kind == TokenKind::RParen {
            balance += 1;
        } else if t.kind == TokenKind::LParen {
            if balance == 0 {
                lparen_idx = Some(i);
                break;
            } else {
                balance -= 1;
            }
        } else if t.kind == TokenKind::LBrace
            || t.kind == TokenKind::RBrace
            || t.kind == TokenKind::Semicolon
        {
            if balance == 0 {
                return None;
            }
        }
    }

    let lparen_idx = lparen_idx?;
    if lparen_idx == 0 {
        return None;
    }

    let mut callee_idx = lparen_idx - 1;
    if state.tokens[callee_idx].kind == TokenKind::LAngle {
        let mut angle_balance = 1;
        for i in (0..callee_idx).rev() {
            if state.tokens[i].kind == TokenKind::RAngle {
                angle_balance += 1;
            } else if state.tokens[i].kind == TokenKind::LAngle {
                angle_balance -= 1;
                if angle_balance == 0 {
                    if i > 0 {
                        callee_idx = i - 1;
                        break;
                    }
                }
            }
        }
    }

    let callee_tok = &state.tokens[callee_idx];

    let mut fn_params = Vec::new();

    if let Some(info) = state.db.expr_types.get(&callee_tok.offset) {
        let ty = &info.ty.0;
        let mut current_ty = ty;
        if let TypeKind::Union(variants) = ty {
            if let Some(t) = variants.iter().find(|v| matches!(v.0, TypeKind::Fn(_))) {
                current_ty = &t.0;
            }
        }

        if let TypeKind::Fn(f) = current_ty {
            for p in &f.params {
                if let Some(name) = &p.name {
                    fn_params.push(name.to_string());
                }
            }
        } else if let TypeKind::Named(name, _) = current_ty {
            if let Some(sym) = state.symbols.iter().find(|s| s.name == name.as_ref()) {
                if let Some(ctor) = sym
                    .members
                    .iter()
                    .find(|m| m.kind == crate::document::MemberKind::Constructor)
                {
                    if let TypeKind::Fn(f) = &ctor.ty.0 {
                        for p in &f.params {
                            if let Some(name) = &p.name {
                                fn_params.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }
    } else if callee_tok.kind == TokenKind::Identifier || callee_tok.kind.can_be_identifier() {
        if let Some(sym) = state.symbols.iter().find(|s| s.name == callee_tok.lexeme) {
            if let TypeKind::Fn(f) = &sym.ty.0 {
                for p in &f.params {
                    if let Some(name) = &p.name {
                        fn_params.push(name.to_string());
                    }
                }
            } else if matches!(sym.kind, varn_checker::SymbolKind::Class) {
                if let Some(ctor) = sym
                    .members
                    .iter()
                    .find(|m| m.kind == crate::document::MemberKind::Constructor)
                {
                    if let TypeKind::Fn(f) = &ctor.ty.0 {
                        for p in &f.params {
                            if let Some(name) = &p.name {
                                fn_params.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    if fn_params.is_empty() {
        return None;
    }

    let mut provided_named_args = FxHashSet::default();
    for i in (lparen_idx + 1)..=current_idx {
        let t = &state.tokens[i];
        if (t.kind == TokenKind::Identifier || t.kind.can_be_identifier())
            && i + 1 < state.tokens.len()
        {
            if state.tokens[i + 1].kind == TokenKind::Colon {
                provided_named_args.insert(t.lexeme.clone());
            }
        }
    }

    let mut items = Vec::new();
    for param in fn_params {
        if !provided_named_args.contains(&param) {
            items.push(CompletionItem {
                label: format!("{}:", param),
                kind: Some(CompletionItemKind::FIELD),
                insert_text: Some(format!("{}: $1", param)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("named argument".to_string()),
                ..Default::default()
            });
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}
