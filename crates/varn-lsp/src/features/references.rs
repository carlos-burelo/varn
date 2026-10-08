use crate::document::{DocumentState, SymbolTarget};
use tower_lsp_f::lsp_types::{Location, Position, Range, Uri};

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
        SymbolTarget::Local { .. } => state.lexeme(token),
        SymbolTarget::Global { canonical_name, .. } => canonical_name.as_str(),
        SymbolTarget::Member { member_name, .. } => member_name.as_str(),
    };

    let mut locs: Vec<Location> = Vec::new();
    let mut checked_uris = rustc_hash::FxHashSet::default();

    if matches!(target, SymbolTarget::Local { .. }) {
        collect_references_in_document(state, &target, target_name, &mut locs);
        return if locs.is_empty() { None } else { Some(locs) };
    }

    let open_entries: Vec<(String, std::sync::Arc<DocumentState>)> = workspace
        .iter()
        .map(|entry| (entry.key().clone(), std::sync::Arc::clone(entry.value())))
        .collect();

    for (file_uri, file_state) in &open_entries {
        checked_uris.insert(file_uri.clone());
        collect_references_in_document(file_state, &target, target_name, &mut locs);
    }

    let mut all_uris: Vec<String> = {
        let guard = workspace.index.read().unwrap_or_else(|e| e.into_inner());
        guard.module_exports.keys().cloned().collect()
    };
    all_uris.sort();

    let mut scanned = 0usize;
    for file_uri in all_uris {
        if checked_uris.contains(&file_uri) {
            continue;
        }
        if scanned >= 50 {
            break;
        }
        let doc_path = crate::document::uri_to_path(&file_uri);
        if std::fs::metadata(&doc_path)
            .map(|m| m.len())
            .unwrap_or(u64::MAX)
            > 256 * 1024
        {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&doc_path) else {
            continue;
        };
        if !source.contains(target_name) {
            continue;
        }
        scanned += 1;
        let temp_state = crate::pipeline::run_pipeline(source, file_uri.clone());
        collect_references_in_document(&temp_state, &target, target_name, &mut locs);
    }

    if locs.is_empty() {
        None
    } else {
        locs.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then_with(|| a.range.start.line.cmp(&b.range.start.line))
        });
        Some(locs)
    }
}

fn collect_references_in_document(
    file_state: &DocumentState,
    target: &SymbolTarget,
    target_name: &str,
    locs: &mut Vec<Location>,
) {
    let Ok(uri) = Uri::parse(&file_state.uri) else {
        return;
    };

    for t in &file_state.tokens {
        if !(t.kind == varn_core::TokenKind::Identifier || t.kind.can_be_identifier()) {
            continue;
        }
        if file_state.lexeme(t) != target_name {
            continue;
        }
        if file_state.symbol_target_at_offset(t.offset).as_ref() != Some(target) {
            continue;
        }
        locs.push(Location::new(
            uri.clone(),
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
