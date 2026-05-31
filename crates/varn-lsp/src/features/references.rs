use crate::document::DocumentState;
use tower_lsp::lsp_types::{Location, Position, Range, Url};
use varn_checker::symbol::SymbolId;

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

    let target_id: SymbolId = state
        .db
        .expr_types
        .get(&token.offset)
        .and_then(|info| info.symbol_id)?;
    let target_key = symbol_global_key_for_id(state, target_id)?;

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

fn symbol_global_key_for_id(state: &DocumentState, id: SymbolId) -> Option<String> {
    if id >= state.db.arena.len() {
        return None;
    }
    let sym = state.db.arena.get(id);
    let name = sym.name.as_ref();
    let kind = sym.kind;
    let origin = sym.origin_module.as_deref();
    let original_name = sym.original_name.as_deref();

    if let Some(origin_mod) = origin {
        let canonical_name = original_name.unwrap_or(name);
        return Some(format!("m:{origin_mod}#{kind:?}:{canonical_name}"));
    }
    Some(format!("u:{}#{kind:?}:{}", state.uri, id))
}

fn token_global_key(state: &DocumentState, offset: u32) -> Option<String> {
    let sid = state
        .db
        .expr_types
        .get(&offset)
        .and_then(|info| info.symbol_id)?;
    symbol_global_key_for_id(state, sid)
}
