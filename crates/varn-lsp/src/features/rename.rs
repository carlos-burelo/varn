use crate::document::{DocumentState, TokenRecord};
use crate::index::ProjectIndex;
use crate::util::converters::range_on_line;
use crate::workspace::Workspace;
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
    state.symbols.iter().find(|s| s.symbol_id == Some(sid))?;
    let range = range_on_line(line, token.col, token.col + token.length);
    Some(PrepareRenameResponse::Range(range))
}

pub fn build_rename(
    state: &DocumentState,
    workspace: &Workspace,
    index: Option<&ProjectIndex>,
    line: u32,
    col: u32,
    new_name: String,
) -> Option<WorkspaceEdit> {
    let token = find_ident_at(state, line, col)?;
    let target_id: Option<SymbolId> = resolve_symbol_id(state, token.offset);
    let old_name = token.lexeme.clone();

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
                if let Some(id) = target_id {
                    resolve_symbol_id(file_state, t.offset) == Some(id)
                } else {
                    t.lexeme == old_name
                }
            })
            .map(|t| TextEdit {
                range: range_on_line(t.line, t.col, t.col + t.length),
                new_text: new_name.clone(),
            })
            .collect();

        if let Some(idx) = index {
            if idx
                .definitions_of(&old_name)
                .iter()
                .any(|(u, _)| u == &file_uri)
            {
                for dep_uri in idx.dependents_of(&file_uri) {
                    if let Some(dep_state) = workspace.get(dep_uri) {
                        let dep_url = match Url::parse(dep_uri) {
                            Ok(u) => u,
                            Err(_) => continue,
                        };
                        let dep_edits = import_name_edits(&dep_state, &old_name, &new_name);
                        if !dep_edits.is_empty() {
                            changes.entry(dep_url).or_default().extend(dep_edits);
                        }
                    }
                }
            }
        }

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

fn import_name_edits(state: &DocumentState, old_name: &str, new_name: &str) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    let mut in_import = false;
    let mut brace_depth = 0i32;

    for tok in &state.tokens {
        match tok.kind {
            TokenKind::Import => {
                in_import = true;
                brace_depth = 0;
            }
            TokenKind::LBrace if in_import => brace_depth += 1,
            TokenKind::RBrace if in_import => {
                brace_depth -= 1;
                if brace_depth <= 0 {
                    in_import = false;
                }
            }
            TokenKind::Semicolon | TokenKind::Newline => {
                if brace_depth == 0 {
                    in_import = false;
                }
            }
            TokenKind::Identifier if in_import && brace_depth > 0 => {
                if tok.lexeme == old_name {
                    edits.push(TextEdit {
                        range: range_on_line(tok.line, tok.col, tok.col + tok.length),
                        new_text: new_name.to_owned(),
                    });
                }
            }
            _ => {}
        }
    }

    edits
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
    state
        .db
        .expr_types
        .get(&offset)
        .and_then(|info| info.symbol_id)
}
