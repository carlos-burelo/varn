use tower_lsp::lsp_types::{CompletionItem, InsertTextFormat};
use varn_checker::SymbolKind;

use crate::document::DocumentState;
use crate::util::converters::to_completion_kind;

pub fn build_scope_completions(state: &DocumentState, line: u32) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();

    let cursor_offset = state.offset_at_line_col(line, 0);
    let mut scope_id = state.db.scope_at_offset(cursor_offset);

    let mut seen_names = std::collections::HashSet::new();

    loop {
        let scope = state.db.scopes.get(scope_id);

        for &symbol_id in &scope.ordered {
            let sym = state.db.arena.get(symbol_id);
            if seen_names.insert(sym.name.to_string()) {
                let ty = state
                    .db
                    .symbol_types
                    .get(&symbol_id)
                    .cloned()
                    .or_else(|| sym.ty.clone())
                    .unwrap_or(varn_checker::types::Type::Dynamic);

                let detail = if ty.is_dynamic() {
                    None
                } else {
                    Some(ty.to_string())
                };

                let (insert_text, insert_text_format) =
                    if sym.kind == SymbolKind::Function {
                        (
                            Some(format!("{}($0)", sym.name)),
                            Some(InsertTextFormat::SNIPPET),
                        )
                    } else {
                        (None, None)
                    };

                items.push(CompletionItem {
                    label: sym.name.to_string(),
                    kind: Some(to_completion_kind(sym.kind)),
                    detail,
                    insert_text,
                    insert_text_format,
                    ..Default::default()
                });
            }
        }

        if let Some(parent) = scope.parent {
            scope_id = parent;
        } else {
            break;
        }
    }

    items
}
