use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};
use varn_checker::SymbolKind;
use varn_core::TypeKind;

use crate::document::DocumentState;
use crate::index::ProjectIndex;

pub fn build_goto_type_definition(
    state: &DocumentState,
    index: Option<&ProjectIndex>,
    line: u32,
    col: u32,
) -> Option<GotoDefinitionResponse> {
    let token = state.identifier_token_at(line, col)?;

    // 1. Resolve symbol or expression type at offset
    let target_type = if let Some(sid) = state.checker_symbol_id_at_token(token) {
        state
            .db
            .symbol_types
            .get(&sid)
            .cloned()
            .or_else(|| state.db.arena.get(sid).ty)
    } else {
        state.db.expr_types.get(&token.offset).map(|info| info.ty)
    }?;

    let type_name = extract_type_identifier(state, &target_type)?;

    // 2. Search for the type definition in the current document
    for sym in state.symbols() {
        if sym.name() == type_name
            && matches!(
                sym.kind(),
                SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Enum
                    | SymbolKind::Struct
                    | SymbolKind::TypeAlias
            )
            && sym.line() != u32::MAX
        {
            let url = Url::parse(&state.uri).ok()?;
            let loc = Location::new(
                url,
                Range {
                    start: Position {
                        line: sym.line(),
                        character: sym.col(),
                    },
                    end: Position {
                        line: sym.end_line(),
                        character: sym.end_col(),
                    },
                },
            );
            return Some(GotoDefinitionResponse::Scalar(loc));
        }
    }

    // 3. Search in ProjectIndex across workspace
    if let Some(idx) = index {
        let defs = idx.definitions_of(&type_name);
        let locs: Vec<Location> = defs
            .iter()
            .filter_map(|(uri, entry)| {
                let url = Url::parse(uri).ok()?;
                Some(Location::new(
                    url,
                    Range {
                        start: Position {
                            line: entry.line,
                            character: entry.col,
                        },
                        end: Position {
                            line: entry.line,
                            character: entry.col + type_name.len() as u32,
                        },
                    },
                ))
            })
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

/// The declaration `ty` names: a named or generic type's own, an array's
/// element type's, the one named member of an optional (`Foo | null`).
fn extract_type_identifier(state: &DocumentState, ty: &varn_checker::Type) -> Option<String> {
    match state.db.ty_kind(ty) {
        TypeKind::Named(name, _) | TypeKind::Generic(name, _, _) => {
            Some(state.name(name).to_owned())
        }
        TypeKind::Array(elem) => extract_type_identifier(state, &varn_checker::Type(elem, false)),
        TypeKind::Union(list) => {
            let named: Vec<String> = state
                .db
                .ty_list(list)
                .iter()
                .filter_map(|t| extract_type_identifier(state, t))
                .collect();
            match named.as_slice() {
                [only] => Some(only.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}
