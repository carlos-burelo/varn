use crate::document::{DocumentState, SymbolTarget};
use tower_lsp::lsp_types::{Location, Position, Range, Url};

pub fn build_references(
    state: &DocumentState,
    workspace: &crate::workspace::Workspace,
    line: u32,
    col: u32,
) -> Option<Vec<Location>> {
    let token = state.tokens.iter().find(|t| {
        t.line == line
            && t.col <= col
            && col < t.col + t.length
            && (t.kind == varn_core::TokenKind::Identifier || t.kind.can_be_identifier())
    })?;

    let target = state.symbol_target_at_offset(token.offset)?;
    let target_name = match &target {
        SymbolTarget::Local { .. } => token.lexeme.as_str(),
        SymbolTarget::Global { canonical_name, .. } => canonical_name.as_str(),
        SymbolTarget::Member { member_name, .. } => member_name.as_str(),
    };

    let mut locs: Vec<Location> = Vec::new();
    let mut checked_uris = rustc_hash::FxHashSet::default();

    // 1. If target is Local, it is scoped to this file only.
    if matches!(target, SymbolTarget::Local { .. }) {
        collect_references_in_document(state, &target, target_name, &mut locs);
        return if locs.is_empty() { None } else { Some(locs) };
    }

    // 2. Global or Member: search open documents in workspace.files first.
    let open_entries: Vec<(String, std::sync::Arc<DocumentState>)> = workspace
        .iter()
        .map(|entry| (entry.key().clone(), std::sync::Arc::clone(entry.value())))
        .collect();

    for (file_uri, file_state) in &open_entries {
        checked_uris.insert(file_uri.clone());
        collect_references_in_document(file_state, &target, target_name, &mut locs);
    }

    // 3. Search unopened files known to the project index on-demand without keeping them in memory.
    let all_uris: Vec<String> = {
        let idx = workspace.index.read().unwrap();
        idx.module_exports.keys().cloned().collect()
    };

    for file_uri in all_uris {
        if checked_uris.contains(&file_uri) {
            continue;
        }
        let doc_path = crate::document::uri_to_path(&file_uri);
        let Ok(source) = std::fs::read_to_string(&doc_path) else {
            continue;
        };
        if !source.contains(target_name) {
            continue;
        }
        let temp_state = crate::pipeline::run_pipeline(source, file_uri.clone());
        collect_references_in_document(&temp_state, &target, target_name, &mut locs);
    }

    if locs.is_empty() {
        None
    } else {
        Some(locs)
    }
}

fn collect_references_in_document(
    file_state: &DocumentState,
    target: &SymbolTarget,
    target_name: &str,
    locs: &mut Vec<Location>,
) {
    let Ok(url) = Url::parse(&file_state.uri) else {
        return;
    };

    for t in &file_state.tokens {
        if !(t.kind == varn_core::TokenKind::Identifier || t.kind.can_be_identifier()) {
            continue;
        }
        if t.lexeme != target_name {
            continue;
        }
        if file_state.symbol_target_at_offset(t.offset).as_ref() != Some(target) {
            continue;
        }
        locs.push(Location::new(
            url.clone(),
            Range {
                start: Position {
                    line: t.line,
                    character: t.col,
                },
                end: Position {
                    line: t.line,
                    character: t.col + t.length,
                },
            },
        ));
    }
}
