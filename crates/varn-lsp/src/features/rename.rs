use crate::document::{DocumentState, TokenRecord};
use crate::util::converters::range_on_line;
use std::collections::HashMap;
use tower_lsp::lsp_types::{PrepareRenameResponse, TextEdit, Url, WorkspaceEdit};
use varn_checker::symbol::SymbolId;
use varn_core::TokenKind;

pub fn build_prepare_rename(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<PrepareRenameResponse> {
    let token = find_ident_at(state, line, col)?;
    let sid = resolve_symbol_id(state, token.offset)?;

    if sid >= state.db.arena.len() {
        return None;
    }
    let sym = state.db.arena.get(sid);
    if sym.origin_module.is_some() {
        return None;
    }

    let range = range_on_line(line, token.col, token.col + token.length);
    Some(PrepareRenameResponse::Range(range))
}

pub fn build_rename(
    state: &DocumentState,
    _workspace: &crate::workspace::Workspace,
    _index: Option<&crate::index::ProjectIndex>,
    line: u32,
    col: u32,
    new_name: String,
) -> Option<WorkspaceEdit> {
    let token = find_ident_at(state, line, col)?;
    let target_id: SymbolId = resolve_symbol_id(state, token.offset)?;
    let target_key = symbol_global_key_for_id(state, target_id)?;

    let target_name = token.lexeme.as_str();
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

    for entry in _workspace.iter() {
        let file_uri = entry.key().clone();
        let file_state = entry.value();
        let url = match Url::parse(&file_uri) {
            Ok(u) => u,
            Err(_) => continue,
        };

        let mut candidate_names = std::collections::HashSet::new();
        candidate_names.insert(target_name.to_owned());
        for sym in &file_state.symbols {
            if sym.global_key == target_key {
                candidate_names.insert(sym.name.clone());
            }
        }

        let mut edits: Vec<TextEdit> = file_state
            .tokens
            .iter()
            .filter(|t| {
                if !matches!(t.kind, TokenKind::Identifier) && !t.kind.can_be_identifier() {
                    return false;
                }
                if !candidate_names.contains(&t.lexeme) {
                    return false;
                }
                token_global_key(file_state, t.offset).as_deref() == Some(target_key.as_str())
            })
            .map(|t| TextEdit {
                range: range_on_line(t.line, t.col, t.col + t.length),
                new_text: new_name.clone(),
            })
            .collect();

        if !edits.is_empty() {
            edits.dedup_by(|a, b| a.range == b.range);
            changes.entry(url).or_default().extend(edits);
        }
    }

    if changes.is_empty() {
        return None;
    }

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

fn find_ident_at(state: &DocumentState, line: u32, col: u32) -> Option<&TokenRecord> {
    state.tokens.iter().find(|t| {
        t.line == line
            && t.col <= col
            && col < t.col + t.length
            && (t.kind == TokenKind::Identifier || t.kind.can_be_identifier())
    })
}

fn resolve_symbol_id(state: &DocumentState, offset: u32) -> Option<SymbolId> {
    state.resolve_symbol_id_at_offset(offset)
}

fn symbol_global_key_for_id(state: &DocumentState, id: SymbolId) -> Option<String> {
    state.symbol_global_key_for_id(id)
}

fn token_global_key(state: &DocumentState, offset: u32) -> Option<String> {
    state.token_global_key(offset)
}
