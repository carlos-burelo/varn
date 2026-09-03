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
            if *symbol_id >= state.db.arena.len() {
                return None;
            }
            let sym = state.db.arena.get(*symbol_id);
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
        SymbolTarget::Local { .. } => token.lexeme.as_str(),
        SymbolTarget::Global { canonical_name, .. } => canonical_name.as_str(),
        SymbolTarget::Member { member_name, .. } => member_name.as_str(),
    };
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

    for entry in workspace.iter() {
        let file_uri = entry.key().clone();
        let file_state = entry.value();
        let url = match Url::parse(&file_uri) {
            Ok(u) => u,
            Err(_) => continue,
        };

        let mut edits: Vec<TextEdit> = file_state
            .tokens
            .iter()
            .filter(|t| {
                if !matches!(t.kind, TokenKind::Identifier) && !t.kind.can_be_identifier() {
                    return false;
                }
                if t.lexeme != target_name {
                    return false;
                }
                file_state.symbol_target_at_offset(t.offset).as_ref() == Some(&target)
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
