use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use varn_checker::SymbolKind;

use crate::constants::{SORT_GLOBAL, SORT_LOCAL, SORT_PARAM};
use crate::document::DocumentState;

pub fn build_scope_completions(state: &DocumentState, line: u32) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();

    let best_scope = state
        .param_scopes
        .iter()
        .filter(|s| line >= s.body_start_line && line <= s.body_end_line)
        .max_by_key(|s| s.body_start_line);

    if let Some(scope) = best_scope {
        for (name, type_str) in &scope.params {
            let detail = if type_str.is_empty() {
                None
            } else {
                Some(type_str.clone())
            };
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail,
                sort_text: Some(format!("{SORT_LOCAL}{name}")),
                ..Default::default()
            });
        }
    }

    let cursor_fn_bodies: Vec<(u32, u32)> = state
        .param_scopes
        .iter()
        .filter(|s| line >= s.body_start_line && line <= s.body_end_line)
        .map(|s| (s.body_start_line, s.body_end_line))
        .collect();

    for sym in &state.symbols {
        if sym.line == u32::MAX {
            continue;
        }
        if sym.line > line {
            continue;
        }

        let sym_in_any_fn = state
            .param_scopes
            .iter()
            .any(|s| sym.line >= s.body_start_line && sym.line <= s.body_end_line);
        if sym_in_any_fn {
            let in_accessible_fn = cursor_fn_bodies
                .iter()
                .any(|(start, end)| sym.line >= *start && sym.line <= *end);
            if !in_accessible_fn {
                continue;
            }
        }

        match sym.kind {
            SymbolKind::Let | SymbolKind::Const | SymbolKind::Var => {
                let detail = if sym.type_str.is_empty() {
                    None
                } else {
                    Some(sym.type_str.clone())
                };
                items.push(CompletionItem {
                    label: sym.name.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail,
                    sort_text: Some(format!("{SORT_PARAM}{}", sym.name)),
                    ..Default::default()
                });
            }
            SymbolKind::Function => {
                let detail = if sym.type_str.is_empty() {
                    None
                } else {
                    Some(sym.type_str.clone())
                };
                items.push(CompletionItem {
                    label: sym.name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail,
                    insert_text: Some(format!("{}($0)", sym.name)),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    sort_text: Some(format!("{SORT_GLOBAL}{}", sym.name)),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }

    items
}
