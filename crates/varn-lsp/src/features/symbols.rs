use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, Range};
use varn_checker::SymbolKind;

use crate::document::{DocumentState, SymbolRecord};
use crate::util::converters::{range_on_line, to_lsp_symbol_kind};

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
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Namespace
            | SymbolKind::Enum
            | SymbolKind::Struct
    )
}

#[allow(deprecated)]
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

#[allow(deprecated)]
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
#[allow(deprecated)]
fn member_to_doc(m: &crate::document::MemberRecord) -> DocumentSymbol {
    let name_end = m.col + m.name.len() as u32;
    // Map MemberKind to SymbolKind
    let kind = match m.kind {
        crate::document::MemberKind::Constructor => SymbolKind::Method,
        crate::document::MemberKind::Method => SymbolKind::Method,
        crate::document::MemberKind::Function => SymbolKind::Function,
        crate::document::MemberKind::Property => SymbolKind::Property,
        crate::document::MemberKind::Variable => SymbolKind::Let,
        crate::document::MemberKind::EnumMember => SymbolKind::EnumMember,
        crate::document::MemberKind::Getter => SymbolKind::Method,
        crate::document::MemberKind::Setter => SymbolKind::Method,
        crate::document::MemberKind::Class => SymbolKind::Class,
        crate::document::MemberKind::Interface => SymbolKind::Interface,
        crate::document::MemberKind::Namespace => SymbolKind::Namespace,
        crate::document::MemberKind::Enum => SymbolKind::Enum,
        crate::document::MemberKind::Struct => SymbolKind::Struct,
    };
    
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
    
    DocumentSymbol {
        name: m.name.clone(),
        detail,
        kind: to_lsp_symbol_kind(kind),
        tags: None,
        deprecated: None,
        range: full_range,
        selection_range: select_range,
        children,
    }
}

#[allow(deprecated)]
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

    DocumentSymbol {
        name: sym.name.clone(),
        detail,
        kind: to_lsp_symbol_kind(sym.kind),
        tags: None,
        deprecated: None,
        range: full_range,
        selection_range: select_range,
        children,
    }
}
