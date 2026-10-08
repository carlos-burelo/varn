use crate::document::DocumentState;
use crate::index::ProjectIndex;
use crate::util::converters::{to_lsp_symbol_kind, zero_range};
use tower_lsp_f::lsp_types::{Position, Range, SymbolKind as LspKind, TypeHierarchyItem, Uri};

pub fn prepare(
    state: &DocumentState,
    index: Option<&ProjectIndex>,
    line: u32,
    col: u32,
) -> Option<Vec<TypeHierarchyItem>> {
    let token = state.identifier_token_at(line, col)?;
    let name = state.lexeme(token).to_owned();
    let mut items = Vec::new();
    if let Some(idx) = index {
        for entry in idx.definitions_of(&name) {
            if !is_typeable(entry.kind) {
                continue;
            }
            items.push(entry_to_item(entry));
        }
    }
    if items.is_empty() {
        for sym in state.symbols() {
            if sym.name() == name && is_typeable(sym.kind()) && sym.line() != u32::MAX {
                let uri = Uri::parse(&state.uri).ok()?;
                items.push(TypeHierarchyItem {
                    name: name.clone(),
                    kind: to_lsp_symbol_kind(sym.kind()),
                    tags: None,
                    detail: None,
                    uri,
                    range: zero_range(sym.line(), sym.col()),
                    selection_range: zero_range(sym.line(), sym.col()),
                    data: None,
                });
                break;
            }
        }
    }
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

pub fn supertypes(
    item: TypeHierarchyItem,
    index: Option<&ProjectIndex>,
) -> Option<Vec<TypeHierarchyItem>> {
    let idx = index?;
    let entry = idx
        .definitions_of(&item.name)
        .iter()
        .find(|e| e.uri.as_ref() == item.uri.as_str() && e.kind == from_lsp_kind(item.kind));
    let parent = entry?.parent.clone()?;
    let mut out = Vec::new();
    for e in idx.definitions_of(parent.as_ref()) {
        out.push(entry_to_item(e));
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn subtypes(
    item: TypeHierarchyItem,
    index: Option<&ProjectIndex>,
) -> Option<Vec<TypeHierarchyItem>> {
    let idx = index?;
    let mut out = Vec::new();
    for entries in idx.name_index.values() {
        for e in entries {
            if e.parent.as_deref() == Some(item.name.as_str()) {
                out.push(entry_to_item(e));
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn entry_to_item(e: &crate::index::ExportEntry) -> TypeHierarchyItem {
    let uri = Uri::parse(e.uri.as_ref()).unwrap_or_else(|_| Uri::parse("file:///unknown").unwrap());
    let pos = Position {
        line: e.line,
        character: e.col,
    };
    let range = Range::new(pos, pos);
    #[allow(deprecated)]
    let _ = LspKind::Class;
    TypeHierarchyItem {
        name: e.name.clone(),
        kind: to_lsp_symbol_kind(e.kind),
        tags: None,
        detail: if e.type_str.is_empty() {
            None
        } else {
            Some(e.type_str.clone())
        },
        uri,
        range,
        selection_range: range,
        data: None,
    }
}

fn is_typeable(k: varn_checker::SymbolKind) -> bool {
    use varn_checker::SymbolKind as S;
    matches!(
        k,
        S::Class | S::Struct | S::Interface | S::Enum | S::TypeAlias
    )
}

fn from_lsp_kind(k: LspKind) -> varn_checker::SymbolKind {
    use varn_checker::SymbolKind as S;
    match k {
        LspKind::Class => S::Class,
        LspKind::Struct => S::Struct,
        LspKind::Interface => S::Interface,
        LspKind::Enum => S::Enum,
        LspKind::File
        | LspKind::Module
        | LspKind::Namespace
        | LspKind::Package
        | LspKind::Method
        | LspKind::Property
        | LspKind::Field
        | LspKind::Constructor
        | LspKind::Function
        | LspKind::Variable
        | LspKind::Constant
        | LspKind::String
        | LspKind::Number
        | LspKind::Boolean
        | LspKind::Array
        | LspKind::Object
        | LspKind::Key
        | LspKind::Null
        | LspKind::EnumMember
        | LspKind::Event
        | LspKind::Operator
        | LspKind::TypeParameter => S::Class,
    }
}
