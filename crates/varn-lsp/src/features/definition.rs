use crate::document::{ChainResult, DocumentState};
use crate::index::ProjectIndex;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};

pub fn build_goto_definition(
    state: &DocumentState,
    index: Option<&ProjectIndex>,
    line: u32,
    col: u32,
) -> Option<GotoDefinitionResponse> {
    let token = state.tokens.iter().find(|t| {
        t.line == line
            && t.col <= col
            && col < t.col + t.length
            && (t.kind == varn_core::TokenKind::Identifier || t.kind.can_be_identifier())
    })?;

    if let Some(chain) = state.resolve_chain_at(line, col) {
        let (member_line, member_col, member_name) = match &chain {
            ChainResult::Member { member, .. } => (member.line, member.col, member.name.clone()),
            ChainResult::DynamicMember { member, .. } => {
                (member.line, member.col, member.name.clone())
            }
            ChainResult::Symbol(_) => (u32::MAX, 0, String::new()),
        };

        if member_line != u32::MAX {
            if let Some(idx) = index {
                let defs = idx.definitions_of(&member_name);
                if !defs.is_empty() {
                    let locs: Vec<Location> = defs
                        .iter()
                        .filter_map(|(uri, entry)| entry_location(uri, entry.line, entry.col))
                        .collect();
                    if !locs.is_empty() {
                        return Some(if locs.len() == 1 {
                            GotoDefinitionResponse::Scalar(locs.into_iter().next().unwrap())
                        } else {
                            GotoDefinitionResponse::Array(locs)
                        });
                    }
                }
            }

            let pos = Position {
                line: member_line,
                character: member_col,
            };
            let range = Range {
                start: pos,
                end: pos,
            };
            let url = Url::parse(&state.uri).ok()?;
            return Some(GotoDefinitionResponse::Scalar(Location::new(url, range)));
        }
    }

    let local = if let Some(info) = state.db.expr_types.get(&token.offset) {
        if let Some(sid) = info.symbol_id {
            state.symbols.iter().find(|s| s.symbol_id == Some(sid))
        } else {
            state.symbols.iter().find(|s| s.name == token.lexeme)
        }
    } else {
        state.symbols.iter().find(|s| s.name == token.lexeme)
    };

    if let Some(sym) = local {
        let pos = Position {
            line: sym.line,
            character: sym.col,
        };
        let range = Range {
            start: pos,
            end: pos,
        };
        let url = Url::parse(&state.uri).ok()?;
        return Some(GotoDefinitionResponse::Scalar(Location::new(url, range)));
    }

    let idx = index?;
    let definitions = idx.definitions_of(&token.lexeme);
    if definitions.is_empty() {
        return None;
    }

    if definitions.len() == 1 {
        let (uri, entry) = &definitions[0];
        let loc = entry_location(uri, entry.line, entry.col)?;
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    let locs: Vec<Location> = definitions
        .iter()
        .filter_map(|(uri, entry)| entry_location(uri, entry.line, entry.col))
        .collect();

    if locs.is_empty() {
        None
    } else {
        Some(GotoDefinitionResponse::Array(locs))
    }
}

fn entry_location(uri: &str, line: u32, col: u32) -> Option<Location> {
    let url = Url::parse(uri).ok()?;
    let pos = Position {
        line,
        character: col,
    };
    Some(Location::new(
        url,
        Range {
            start: pos,
            end: pos,
        },
    ))
}
