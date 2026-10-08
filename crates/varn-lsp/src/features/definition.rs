use crate::document::{ChainResult, DocumentState};
use crate::index::ProjectIndex;
use crate::util::converters::zero_range;
use tower_lsp_f::lsp_types::{Definition, Location, Position, Range, Uri};
use varn_checker::symbol::SymbolId;
pub fn build_goto_definition(
    state: &DocumentState,
    index: Option<&ProjectIndex>,
    line: u32,
    col: u32,
) -> Option<Definition> {
    let token = state.identifier_token_at(line, col)?;
    if let Some(mem_res) = state.db.member_resolutions.get(&token.offset) {
        if let Some(def_range) = mem_res.def_range {
            if let Ok(uri) = Uri::parse(&state.uri) {
                return Some(Definition::Location(Location::new(
                    uri,
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
                return Some(Definition::Location(loc));
            }
        }
    }
    if let Some(sid) = state.checker_symbol_id_at_token(token) {
        if let Some(loc) = resolve_symbol_location(state, sid) {
            return Some(Definition::Location(loc));
        }
    }
    if let Some(chain) = state.resolve_chain_at(line, col) {
        match chain {
            ChainResult::Member {
                member,
                parent_name,
            } => {
                if let Some(loc) = resolve_member_location(index, &parent_name, &member.name) {
                    return Some(Definition::Location(loc));
                }
                if let Some(line) = member.def_line {
                    let uri = Uri::parse(&state.uri).ok()?;
                    return Some(Definition::Location(Location::new(
                        uri,
                        zero_range(line.saturating_sub(1), member.def_col),
                    )));
                }
            }
            ChainResult::Symbol(sym_rec) => {
                if let Some(loc) = resolve_symbol_location(state, sym_rec.id) {
                    return Some(Definition::Location(loc));
                }
            }
        }
    }
    if let Some(idx) = index {
        let defs = idx.definitions_of(state.lexeme(token));
        let locs: Vec<Location> = defs
            .iter()
            .filter_map(|entry| entry_location(&entry.uri, entry.line, entry.col))
            .collect();
        if !locs.is_empty() {
            return Some(if locs.len() == 1 {
                let mut locs = locs;
                Definition::Location(locs.swap_remove(0))
            } else {
                Definition::LocationList(locs)
            });
        }
    }
    None
}
fn resolve_symbol_location(state: &DocumentState, sid: SymbolId) -> Option<Location> {
    if sid >= state.db.bind.arena.len() {
        return None;
    }
    let sym = state.db.bind.arena.get(sid);
    let uri = if let Some(origin) = &sym.origin_module {
        resolve_origin_to_url(state.name(*origin))?
    } else {
        Uri::parse(&state.uri).ok()?
    };
    let line = sym.line.saturating_sub(1);
    let pos = Position {
        line,
        character: sym.col,
    };
    Some(Location::new(uri, zero_range(pos.line, pos.character)))
}
fn resolve_origin_to_url(origin: &str) -> Option<Uri> {
    if origin.starts_with("file://") {
        return Uri::parse(origin).ok();
    }
    if std::path::Path::new(origin).is_absolute() {
        return Uri::from_file_path(origin).ok();
    }
    let path = crate::workspace::std_sources::resolve_module_file(origin)?;
    Uri::from_file_path(path).ok()
}
fn entry_location(uri: &str, line: u32, col: u32) -> Option<Location> {
    let uri = Uri::parse(uri).ok()?;
    let pos = Position {
        line,
        character: col,
    };
    Some(Location::new(uri, zero_range(pos.line, pos.character)))
}
fn resolve_member_location(
    index: Option<&ProjectIndex>,
    parent_name: &str,
    member_name: &str,
) -> Option<Location> {
    let idx = index?;
    let entries = idx.definitions_of(member_name);
    let entry_opt = entries
        .iter()
        .find(|entry| entry.name == member_name && entry.parent.as_deref() == Some(parent_name));
    if let Some(entry) = entry_opt {
        let uri = Uri::parse(&entry.uri).ok()?;
        let pos = Position {
            line: entry.line,
            character: entry.col,
        };
        return Some(Location::new(uri, Range::new(pos, pos)));
    }
    None
}
