use crate::document::{DocumentState, SymbolTarget, TokenRecord};
use crate::util::converters::range_on_line;
use std::collections::HashMap;
use tower_lsp::lsp_types::{PrepareRenameResponse, TextEdit, Url, WorkspaceEdit};
use varn_core::TokenKind;

pub fn build_prepare_rename(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<PrepareRenameResponse> {
    let token = find_ident_at(state, line, col)?;
    let target = state.symbol_target_at_offset(token.offset)?;

    match &target {
        SymbolTarget::Local { symbol_id, .. } => {
            if *symbol_id >= state.db.bind.arena.len() {
                return None;
            }
            let sym = state.db.bind.arena.get(*symbol_id);
            if sym.origin_module.is_some() {
                return None;
            }
        }
        SymbolTarget::Global { origin, .. } => {
            if origin.starts_with("std:")
                || origin.starts_with("core:")
                || origin.starts_with("runtime:")
            {
                return None;
            }
        }
        SymbolTarget::Member { .. } => {}
    }

    let range = range_on_line(line, token.col, token.col + token.length);
    Some(PrepareRenameResponse::Range(range))
}

pub fn build_rename(
    state: &DocumentState,
    workspace: &crate::workspace::Workspace,
    _index: Option<&crate::index::ProjectIndex>,
    line: u32,
    col: u32,
    new_name: String,
) -> Option<WorkspaceEdit> {
    let token = find_ident_at(state, line, col)?;
    let target = state.symbol_target_at_offset(token.offset)?;

    let target_name = match &target {
        SymbolTarget::Local { .. } => state.lexeme(token),
        SymbolTarget::Global { canonical_name, .. } => canonical_name.as_str(),
        SymbolTarget::Member { member_name, .. } => member_name.as_str(),
    };
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

    if matches!(target, SymbolTarget::Local { .. }) {
        collect_rename_edits_in_document(state, &target, target_name, &new_name, &mut changes);
        return if changes.is_empty() {
            None
        } else {
            Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            })
        };
    }

    let mut checked_uris = rustc_hash::FxHashSet::default();

    let open_entries: Vec<(String, std::sync::Arc<DocumentState>)> = workspace
        .iter()
        .map(|entry| (entry.key().clone(), std::sync::Arc::clone(entry.value())))
        .collect();

    for (file_uri, file_state) in &open_entries {
        checked_uris.insert(file_uri.clone());
        collect_rename_edits_in_document(file_state, &target, target_name, &new_name, &mut changes);
    }

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
        collect_rename_edits_in_document(
            &temp_state,
            &target,
            target_name,
            &new_name,
            &mut changes,
        );
    }

    if changes.is_empty() {
        return None;
    }

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

fn collect_rename_edits_in_document(
    file_state: &DocumentState,
    target: &SymbolTarget,
    target_name: &str,
    new_name: &str,
    changes: &mut HashMap<Url, Vec<TextEdit>>,
) {
    let Ok(url) = Url::parse(&file_state.uri) else {
        return;
    };

    let mut edits: Vec<TextEdit> = file_state
        .tokens
        .iter()
        .filter(|t| {
            if !matches!(t.kind, TokenKind::Identifier) && !t.kind.can_be_identifier() {
                return false;
            }
            if file_state.lexeme(t) != target_name {
                return false;
            }
            file_state.symbol_target_at_offset(t.offset).as_ref() == Some(target)
        })
        .map(|t| TextEdit {
            range: range_on_line(t.line, t.col, t.col + t.length),
            new_text: new_name.to_string(),
        })
        .collect();

    if !edits.is_empty() {
        edits.dedup_by(|a, b| a.range == b.range);
        changes.entry(url).or_default().extend(edits);
    }
}

fn find_ident_at(state: &DocumentState, line: u32, col: u32) -> Option<&TokenRecord> {
    state.tokens.iter().find(|t| {
        t.line == line
            && t.col <= col
            && col < t.col + t.length
            && (t.kind == TokenKind::Identifier || t.kind.can_be_identifier())
    })
}
