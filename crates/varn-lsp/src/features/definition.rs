use crate::document::{ChainResult, DocumentState};
use crate::index::ProjectIndex;
use crate::util::converters::zero_range;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};
use varn_checker::symbol::SymbolId;

pub fn build_goto_definition(
    state: &DocumentState,
    index: Option<&ProjectIndex>,
    line: u32,
    col: u32,
) -> Option<GotoDefinitionResponse> {
    let token = state.identifier_token_at(line, col)?;

    // 0. Direct MemberResolution
    if let Some(mem_res) = state.db.member_resolutions.get(&token.offset) {
        if let Some(def_range) = mem_res.def_range {
            if let Ok(url) = Url::parse(&state.uri) {
                return Some(GotoDefinitionResponse::Scalar(Location::new(
                    url,
                    Range {
                        start: Position {
                            line: def_range.start.line,
                            character: def_range.start.column,
                        },
                        end: Position {
                            line: def_range.end.line,
                            character: def_range.end.column,
                        },
                    },
                )));
            }
        }
        if let Some(origin_mod) = &mem_res.origin_module {
            if let Some(loc) = resolve_member_location(index, origin_mod, &mem_res.member_name) {
                return Some(GotoDefinitionResponse::Scalar(loc));
            }
        }
    }

    if let Some(sid) = state.checker_symbol_id_at_token(token) {
        if let Some(loc) = resolve_symbol_location(state, sid) {
            return Some(GotoDefinitionResponse::Scalar(loc));
        }
    }

    // 2. Resolve members and dynamic chains.
    if let Some(chain) = state.resolve_chain_at(line, col) {
        match chain {
            ChainResult::Member {
                member,
                parent_name,
            } => {
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
                    return Some(GotoDefinitionResponse::Scalar(Location::new(
                        url,
                        zero_range(pos.line, pos.character),
                    )));
                }
            }
            ChainResult::DynamicMember {
                member,
                parent_name,
            } => {
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
                    return Some(GotoDefinitionResponse::Scalar(Location::new(
                        url,
                        zero_range(pos.line, pos.character),
                    )));
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

    // 3. ProjectIndex remains only as a cross-module fallback.
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
    Some(Location::new(url, zero_range(pos.line, pos.character)))
}

fn resolve_origin_to_url(origin: &str) -> Option<Url> {
    if origin.starts_with("file://") {
        return Url::parse(origin).ok();
    }
    if std::path::Path::new(origin).is_absolute() {
        return Url::from_file_path(origin).ok();
    }
    // Standard library, core or runtime module: an active std tree if there
    // is one, otherwise this binary's own sources mirrored to disk.
    let path = crate::workspace::std_sources::resolve_module_file(origin)?;
    Url::from_file_path(path).ok()
}

fn entry_location(uri: &str, line: u32, col: u32) -> Option<Location> {
    let url = Url::parse(uri).ok()?;
    let pos = Position {
        line,
        character: col,
    };
    Some(Location::new(url, zero_range(pos.line, pos.character)))
}

fn resolve_member_location(
    index: Option<&ProjectIndex>,
    parent_name: &str,
    member_name: &str,
) -> Option<Location> {
    let idx = index?;
    let entries = idx.definitions_of(member_name);
    let prefix = format!("member:{parent_name}:{member_name}:");
    let entry_opt = entries
        .iter()
        .find(|(_, entry)| entry.global_key.starts_with(&prefix));
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
