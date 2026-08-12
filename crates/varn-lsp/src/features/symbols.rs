use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, Range};
use varn_checker::SymbolKind;

use crate::document::{DocumentState, SymbolRecord};
use crate::util::converters::{range_on_line, to_lsp_symbol_kind};
use crate::util::kinds::{is_container_symbol_kind, member_to_symbol_kind};

pub fn build_document_symbols(state: &DocumentState) -> DocumentSymbolResponse {
    let mut sorted: Vec<&SymbolRecord> = state
        .symbols
        .iter()
        .filter(|s| s.line != u32::MAX && !s.is_from_stdlib)
        .collect();
    sorted.sort_by_key(|s| (s.line, s.col));

    DocumentSymbolResponse::Nested(nest_symbols(&sorted))
}

fn is_container(kind: SymbolKind) -> bool {
    is_container_symbol_kind(kind)
}

fn nest_symbols(sorted: &[&SymbolRecord]) -> Vec<DocumentSymbol> {
    let mut container_stack: Vec<u32> = Vec::new();
    let mut roots: Vec<DocumentSymbol> = Vec::new();

    for sym in sorted {
        while let Some(&end_line) = container_stack.last() {
            if sym.line > end_line {
                container_stack.pop();
            } else {
                break;
            }
        }

        let depth = container_stack.len();
        let doc_sym = sym_to_doc(sym);

        if is_container(sym.kind) {
            let end_line = if sym.end_line > sym.line {
                sym.end_line
            } else {
                sym.line
            };

            insert_at_depth(&mut roots, depth, doc_sym);
            container_stack.push(end_line);
        } else {
            insert_at_depth(&mut roots, depth, doc_sym);
        }
    }

    roots
}

fn insert_at_depth(nodes: &mut Vec<DocumentSymbol>, depth: usize, sym: DocumentSymbol) {
    if depth == 0 {
        nodes.push(sym);
        return;
    }
    if let Some(last) = nodes.last_mut() {
        let children = last.children.get_or_insert_with(Vec::new);
        insert_at_depth(children, depth - 1, sym);
    } else {
        nodes.push(sym);
    }
}
fn member_to_doc(m: &crate::document::MemberRecord) -> DocumentSymbol {
    let name_end = m.col + m.name.len() as u32;
    let kind = member_to_symbol_kind(m.kind);

    let full_range = range_on_line(m.line, m.col, name_end);
    let select_range = range_on_line(m.line, m.col, name_end);

    let detail = if m.type_str.is_empty() {
        None
    } else {
        Some(m.type_str.clone())
    };

    let children = if m.members.is_empty() {
        None
    } else {
        Some(m.members.iter().map(member_to_doc).collect())
    };

    // The spec replaced `deprecated` with `tags`, and we do use `tags` — but
    // `lsp_types` implements no `Default` for `DocumentSymbol`, so the struct
    // cannot be built without naming every field, deprecated ones included.
    // The allow is scoped to this literal, not the function, so it cannot
    // silently cover a second deprecation later.
    #[allow(deprecated)]
    let symbol = DocumentSymbol {
        name: m.name.clone(),
        detail,
        kind: to_lsp_symbol_kind(kind),
        tags: None,
        deprecated: None,
        range: full_range,
        selection_range: select_range,
        children,
    };
    symbol
}

fn sym_to_doc(sym: &SymbolRecord) -> DocumentSymbol {
    let name_end = sym.col + sym.name.len() as u32;

    let full_range = if sym.end_line > sym.line {
        Range {
            start: tower_lsp::lsp_types::Position {
                line: sym.line,
                character: 0,
            },
            end: tower_lsp::lsp_types::Position {
                line: sym.end_line,
                character: sym.end_col,
            },
        }
    } else {
        range_on_line(sym.line, 0, name_end)
    };

    let select_range = range_on_line(sym.line, sym.col, name_end);

    let detail = if sym.type_str.is_empty() {
        None
    } else {
        Some(sym.type_str.clone())
    };

    let children = if sym.members.is_empty() {
        None
    } else {
        Some(sym.members.iter().map(member_to_doc).collect())
    };

    // See `member_to_doc` for why the deprecated field is still named here.
    #[allow(deprecated)]
    let symbol = DocumentSymbol {
        name: sym.name.clone(),
        detail,
        kind: to_lsp_symbol_kind(sym.kind),
        tags: None,
        deprecated: None,
        range: full_range,
        selection_range: select_range,
        children,
    };
    symbol
}
