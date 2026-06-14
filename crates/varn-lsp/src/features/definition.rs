use crate::document::{ChainResult, DocumentState};
use crate::index::ProjectIndex;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};
use varn_checker::symbol::SymbolId;

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

    // 1. Try to resolve via direct SymbolId in expr_types
    if let Some(info) = state.db.expr_types.get(&token.offset) {
        if let Some(sid) = info.symbol_id {
            if let Some(loc) = resolve_symbol_location(state, sid) {
                return Some(GotoDefinitionResponse::Scalar(loc));
            }
        }
    }

    // 2. Try to resolve via resolve_chain_at
    if let Some(chain) = state.resolve_chain_at(line, col) {
        match chain {
            ChainResult::Member { member, parent_name } => {
                if let Some(loc) = resolve_member_location(index, &parent_name, &member.name) {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
                if member.line != u32::MAX {
                    if let Some(sid) = member.symbol_id {
                        if let Some(loc) = resolve_symbol_location(state, sid) {
                            return Some(GotoDefinitionResponse::Scalar(loc));
                        }
                    }
                    let pos = Position {
                        line: member.line,
                        character: member.col,
                    };
                    let url = Url::parse(&state.uri).ok()?;
                    return Some(GotoDefinitionResponse::Scalar(Location::new(url, Range::new(pos, pos))));
                }
            }
            ChainResult::DynamicMember { member, parent_name } => {
                if let Some(loc) = resolve_member_location(index, &parent_name, &member.name) {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
                if member.line != u32::MAX {
                    if let Some(sid) = member.symbol_id {
                        if let Some(loc) = resolve_symbol_location(state, sid) {
                            return Some(GotoDefinitionResponse::Scalar(loc));
                        }
                    }
                    let pos = Position {
                        line: member.line,
                        character: member.col,
                    };
                    let url = Url::parse(&state.uri).ok()?;
                    return Some(GotoDefinitionResponse::Scalar(Location::new(url, Range::new(pos, pos))));
                }
            }
            ChainResult::Symbol(sym_rec) => {
                if let Some(sid) = sym_rec.symbol_id {
                    if let Some(loc) = resolve_symbol_location(state, sid) {
                        return Some(GotoDefinitionResponse::Scalar(loc));
                    }
                }
            }
        }
    }

    // 3. Try to resolve via resolve_at (scope lookup)
    if let Some((sid, _)) = state.db.resolve_at(&token.lexeme, token.offset) {
        if let Some(loc) = resolve_symbol_location(state, sid) {
            return Some(GotoDefinitionResponse::Scalar(loc));
        }
    }

    // 4. Try ProjectIndex key search as a last resort
    if let Some(idx) = index {
        let defs = idx.definitions_of(&token.lexeme);
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

    None
}

fn resolve_symbol_location(state: &DocumentState, sid: SymbolId) -> Option<Location> {
    if sid >= state.db.arena.len() {
        return None;
    }
    let sym = state.db.arena.get(sid);

    let url = if let Some(origin) = &sym.origin_module {
        resolve_origin_to_url(origin)?
    } else {
        Url::parse(&state.uri).ok()?
    };

    let line = sym.line.saturating_sub(1);
    let pos = Position {
        line,
        character: sym.col,
    };
    Some(Location::new(url, Range::new(pos, pos)))
}

fn resolve_origin_to_url(origin: &str) -> Option<Url> {
    if origin.starts_with("file://") {
        return Url::parse(origin).ok();
    }
    if std::path::Path::new(origin).is_absolute() {
        return Url::from_file_path(origin).ok();
    }
    // Embedded standard library or core modules
    let provider = varn_modules::provider::get()?;
    if let Some(mod_path) = provider.source_path(origin) {
        if mod_path.is_file() {
            let canonical = std::fs::canonicalize(&mod_path).ok()?;
            return Url::from_file_path(canonical).ok();
        }
    }
    if provider.embedded_source(origin).is_some() {
        return Url::parse(&varn_modules::resolver::to_varn_uri(origin)).ok();
    }
    None
}

fn entry_location(uri: &str, line: u32, col: u32) -> Option<Location> {
    let url = Url::parse(uri).ok()?;
    let pos = Position {
        line,
        character: col,
    };
    Some(Location::new(url, Range::new(pos, pos)))
}

fn resolve_member_location(
    index: Option<&ProjectIndex>,
    parent_name: &str,
    member_name: &str,
) -> Option<Location> {
    let idx = index?;
    let entries = idx.definitions_of(member_name);
    let prefix = format!("member:{parent_name}:{member_name}:");
    let entry_opt = entries.iter().find(|(_, entry)| entry.global_key.starts_with(&prefix));
    if let Some((uri, entry)) = entry_opt {
        let url = Url::parse(uri).ok()?;
        let pos = Position {
            line: entry.line,
            character: entry.col,
        };
        return Some(Location::new(url, Range::new(pos, pos)));
    }
    None
}
