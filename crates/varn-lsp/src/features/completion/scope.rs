use tower_lsp::lsp_types::{CompletionItem, InsertTextFormat};
use varn_checker::SymbolKind;

use crate::document::DocumentState;
use crate::util::converters::to_completion_kind;

pub fn build_scope_completions(state: &DocumentState, line: u32, col: u32) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();

    let cursor_offset = state.offset_at_line_col(line, col);
    let mut scope_id = state.db.scope_at_offset(cursor_offset);

    let mut seen_names = std::collections::HashSet::new();

    loop {
        let scope = state.db.scopes.get(scope_id);

        for &symbol_id in &scope.ordered {
            let sym = state.db.arena.get(symbol_id);
            let name = state.name(sym.name);
            if seen_names.insert(name) {
                let ty = state
                    .db
                    .symbol_types
                    .get(&symbol_id)
                    .cloned()
                    .or(sym.ty)
                    .unwrap_or_default();

                let detail = (!state.db.is_dynamic(&ty)).then(|| state.ty_text(&ty));

                let (insert_text, insert_text_format) = if sym.kind == SymbolKind::Function {
                    (Some(format!("{name}($0)")), Some(InsertTextFormat::SNIPPET))
                } else {
                    (None, None)
                };

                items.push(CompletionItem {
                    label: name.to_owned(),
                    kind: Some(to_completion_kind(sym.kind)),
                    detail,
                    insert_text,
                    insert_text_format,
                    sort_text: Some(format!("0_{name}")),
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
