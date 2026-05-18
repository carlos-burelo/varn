use crate::document::DocumentState;
use crate::workspace::Workspace;
use tower_lsp::lsp_types::{Location, Position, Range, Url};
use varn_checker::symbol::SymbolId;

pub fn build_references(
    state: &DocumentState,
    workspace: &Workspace,
    line: u32,
    col: u32,
) -> Option<Vec<Location>> {
    let token = state.tokens.iter().find(|t| {
        t.line == line
            && t.col <= col
            && col < t.col + t.length
            && (t.kind == varn_core::TokenKind::Identifier || t.kind.can_be_identifier())
    })?;

    let target_id: Option<SymbolId> = state
        .db
        .expr_types
        .get(&token.offset)
        .and_then(|info| info.symbol_id);
    let name = token.lexeme.clone();

    let mut locs: Vec<Location> = Vec::new();

    for entry in workspace.iter() {
        let file_uri = entry.key().clone();
        let file_state = entry.value();
        let url = match Url::parse(&file_uri) {
            Ok(u) => u,
            Err(_) => continue,
        };

        for t in &file_state.tokens {
            if !(t.kind == varn_core::TokenKind::Identifier || t.kind.can_be_identifier()) {
                continue;
            }
            let matches = if let Some(id) = target_id {
                file_state
                    .db
                    .expr_types
                    .get(&t.offset)
                    .and_then(|info| info.symbol_id)
                    == Some(id)
            } else {
                t.lexeme == name
            };
            if !matches {
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

    if locs.is_empty() {
        None
    } else {
        Some(locs)
    }
}
