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

    let entries: Vec<(String, std::sync::Arc<DocumentState>)> = workspace
        .iter()
        .map(|entry| (entry.key().clone(), std::sync::Arc::clone(entry.value())))
        .collect();

    for (file_uri, file_state) in &entries {
        let url = match Url::parse(file_uri) {
            Ok(u) => u,
            Err(_) => continue,
        };

        for t in &file_state.tokens {
            if !(t.kind == varn_core::TokenKind::Identifier || t.kind.can_be_identifier()) {
                continue;
            }
            if t.lexeme != target_name {
                continue;
            }
            if file_state.symbol_target_at_offset(t.offset).as_ref() != Some(&target) {
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
