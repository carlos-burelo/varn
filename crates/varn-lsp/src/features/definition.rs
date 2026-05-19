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
        let (member_line, member_col, member_key) = match &chain {
            ChainResult::Member { member, parent_name } => (
                member.line,
                member.col,
                Some(member_global_key(parent_name, member)),
            ),
            ChainResult::DynamicMember { member, .. } => {
                (member.line, member.col, None)
            }
            ChainResult::Symbol(_) => (u32::MAX, 0, None),
        };

        if member_line != u32::MAX {
            if let (Some(idx), Some(key)) = (index, member_key.as_deref()) {
                let defs = idx.definitions_of_key(key);
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

    let local = state
        .db
        .expr_types
        .get(&token.offset)
        .and_then(|info| info.symbol_id)
        .and_then(|sid| state.symbols.iter().find(|s| s.symbol_id == Some(sid)));

    if let Some(sym) = local {
        if let Some(idx) = index {
            let defs = idx.definitions_of_key(&sym.global_key);
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

    None
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

fn member_global_key(parent_name: &str, member: &crate::document::MemberRecord) -> String {
    let sid = member
        .symbol_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "none".to_owned());
    format!("member:{parent_name}:{}:{sid}", member.name)
}
