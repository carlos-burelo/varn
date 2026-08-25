use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, Range};
use varn_checker::SymbolKind;

use crate::document::{DocumentState, SymbolView};
use crate::util::converters::{range_on_line, to_lsp_symbol_kind};
use crate::util::kinds::is_container_symbol_kind;

pub fn build_document_symbols(state: &DocumentState) -> DocumentSymbolResponse {
    let mut sorted: Vec<SymbolView<'_>> = state
        .symbols()
        .filter(|s| s.line() != u32::MAX && !s.is_from_stdlib())
        .collect();
    sorted.sort_by_key(|s| (s.line(), s.col()));

    DocumentSymbolResponse::Nested(nest_symbols(state, &sorted))
}

fn is_container(kind: SymbolKind) -> bool {
    is_container_symbol_kind(kind)
}

fn nest_symbols(state: &DocumentState, sorted: &[SymbolView<'_>]) -> Vec<DocumentSymbol> {
    let mut container_stack: Vec<u32> = Vec::new();
    let mut roots: Vec<DocumentSymbol> = Vec::new();

    for sym in sorted {
        while let Some(&end_line) = container_stack.last() {
            if sym.line() > end_line {
                container_stack.pop();
            } else {
                break;
            }
        }

        let depth = container_stack.len();
        let doc_sym = sym_to_doc(state, *sym);

        if is_container(sym.kind()) {
            let end_line = if sym.end_line() > sym.line() {
                sym.end_line()
            } else {
                sym.line()
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
/// One member of a type, as an outline node.
///
/// Built from the checker's summary. Only members the checker located are
/// shown: one with no `def_line` has no source of its own — read out of a
/// precompiled interface, or synthesised — and an outline entry that jumps
/// nowhere is worse than no entry.
fn summary_to_doc(m: &varn_checker::ResolvedMemberSummary) -> Option<DocumentSymbol> {
    let line = m.def_line?.saturating_sub(1);
    let name_end = m.def_col + m.name.chars().count() as u32;
    let kind = summary_to_symbol_kind(m.kind);
    let range = range_on_line(line, m.def_col, name_end);

    // See the note in `sym_to_doc` for why the deprecated field is named.
    #[allow(deprecated)]
    Some(DocumentSymbol {
        name: m.name.to_string(),
        detail: Some(m.ty.to_string()),
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children: None,
    })
}

fn summary_to_symbol_kind(
    k: varn_checker::ResolvedMemberKind,
) -> tower_lsp::lsp_types::SymbolKind {
    use tower_lsp::lsp_types::SymbolKind as L;
    use varn_checker::ResolvedMemberKind as R;
    match k {
        R::Method | R::StaticMethod | R::ExtensionMethod => L::METHOD,
        R::EnumMember => L::ENUM_MEMBER,
        _ => L::PROPERTY,
    }
}


fn sym_to_doc(state: &DocumentState, sym: SymbolView<'_>) -> DocumentSymbol {
    let name_end = sym.col() + sym.name().len() as u32;

    let full_range = if sym.end_line() > sym.line() {
        Range {
            start: tower_lsp::lsp_types::Position {
                line: sym.line(),
                character: 0,
            },
            end: tower_lsp::lsp_types::Position {
                line: sym.end_line(),
                character: sym.end_col(),
            },
        }
    } else {
        range_on_line(sym.line(), 0, name_end)
    };

    let select_range = range_on_line(sym.line(), sym.col(), name_end);

    let detail = if sym.type_str().is_empty() {
        None
    } else {
        Some(sym.type_str())
    };

    // Only a container nests. A `const w: Widget` is one entry in the outline,
    // not a folder holding every member of `Widget` — and a `const s: str`
    // would otherwise hang all forty-odd methods of `str` under itself.
    let members: Vec<DocumentSymbol> = if is_container(sym.kind()) {
        state
            .members_of(sym)
            .iter()
            .filter_map(summary_to_doc)
            .collect()
    } else {
        Vec::new()
    };
    let children = if members.is_empty() {
        None
    } else {
        Some(members)
    };

    // See `member_to_doc` for why the deprecated field is still named here.
    #[allow(deprecated)]
    let symbol = DocumentSymbol {
        name: sym.name().to_owned(),
        detail,
        kind: to_lsp_symbol_kind(sym.kind()),
        tags: None,
        deprecated: None,
        range: full_range,
        selection_range: select_range,
        children,
    };
    symbol
}
