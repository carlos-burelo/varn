use crate::document::DocumentState;
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

    // Derive the target with the *same* function that derives the candidates.
    //
    // These used to disagree: the target came from `symbol_global_key_for_id`,
    // which yields the `u:`/`m:` form, while each candidate came from
    // `token_global_key`, which yields a `member:{type}:{name}` key when the
    // token is a class member. For a member the two shapes can never compare
    // equal, so "find all references" on a field or method returned nothing at
    // all — while it worked on a top-level function, where both shapes coincide.
    //
    // Symmetry is the fix: one function, so both sides agree by construction.
    let target_key = state.token_global_key(token.offset)?;
    let target_name = token.lexeme.as_str();

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
        let mut candidate_names = std::collections::HashSet::new();
        candidate_names.insert(target_name.to_owned());
        for sym in file_state.symbols() {
            if sym.global_key(true) == target_key || sym.global_key(false) == target_key {
                candidate_names.insert(sym.name().to_owned());
            }
        }
        for t in &file_state.tokens {
            if !(t.kind == varn_core::TokenKind::Identifier || t.kind.can_be_identifier()) {
                continue;
            }
            if !candidate_names.contains(&t.lexeme) {
                continue;
            }
            if token_global_key(file_state, t.offset).as_deref() != Some(target_key.as_str()) {
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


fn token_global_key(state: &DocumentState, offset: u32) -> Option<String> {
    state.token_global_key(offset)
}
