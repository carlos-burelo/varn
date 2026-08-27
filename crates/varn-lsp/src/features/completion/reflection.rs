use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, InsertTextFormat, MarkupContent, MarkupKind,
};

use crate::document::DocumentState;

pub struct MetaProperty {
    pub name: &'static str,
    pub kind: CompletionItemKind,
    pub detail: &'static str,
    pub doc: &'static str,
    pub snippet: Option<&'static str>,
}

pub static META_PROPERTIES: &[MetaProperty] = &[
    MetaProperty {
        name: "name",
        kind: CompletionItemKind::PROPERTY,
        detail: "Reflection: type / class name (str)",
        doc: "Returns the name of the class or type as a `str`.",
        snippet: None,
    },
    MetaProperty {
        name: "fields",
        kind: CompletionItemKind::PROPERTY,
        detail: "Reflection: field names (str[])",
        doc: "Returns an array of field names declared on this type:\n```varn\nSampleUser::fields // [\"name\", \"age\", \"role\"]\n```",
        snippet: None,
    },
    MetaProperty {
        name: "methods",
        kind: CompletionItemKind::PROPERTY,
        detail: "Reflection: method names (str[])",
        doc: "Returns an array of method names declared on this type:\n```varn\nSampleUser::methods // [\"greet\", \"save\"]\n```",
        snippet: None,
    },
    MetaProperty {
        name: "type",
        kind: CompletionItemKind::PROPERTY,
        detail: "Reflection: type tag (str)",
        doc: "Returns the intrinsic or user type tag string.",
        snippet: None,
    },
    MetaProperty {
        name: "class",
        kind: CompletionItemKind::PROPERTY,
        detail: "Reflection: class constructor reference",
        doc: "Returns the runtime class constructor object.",
        snippet: None,
    },
    MetaProperty {
        name: "keys",
        kind: CompletionItemKind::METHOD,
        detail: "Reflection: keys() -> str[]",
        doc: "Returns the reflection keys of this type.",
        snippet: Some("keys()$0"),
    },
    MetaProperty {
        name: "values",
        kind: CompletionItemKind::METHOD,
        detail: "Reflection: values() -> dynamic[]",
        doc: "Returns the property values of this type.",
        snippet: Some("values()$0"),
    },
    MetaProperty {
        name: "entries",
        kind: CompletionItemKind::METHOD,
        detail: "Reflection: entries() -> [str, dynamic][]",
        doc: "Returns key-value pairs of this type.",
        snippet: Some("entries()$0"),
    },
    MetaProperty {
        name: "hasOwn",
        kind: CompletionItemKind::METHOD,
        detail: "Reflection: hasOwn(key: str) -> bool",
        doc: "Determines whether this type defines the specified property.",
        snippet: Some("hasOwn(\"${1:key}\")$0"),
    },
];

pub fn colon_colon_receiver(
    state: &DocumentState,
    line: u32,
    col: u32,
    _trigger_char: Option<&str>,
) -> Option<String> {
    let line_str = state.source.lines().nth(line as usize)?;
    let col_idx = (col as usize).min(line_str.len());
    let prefix = &line_str[..col_idx];

    let cc_pos = prefix.rfind("::")?;
    let receiver_str = prefix[..cc_pos].trim();
    if receiver_str.is_empty() {
        return None;
    }

    // Extract identifier before ::
    let receiver_ident = receiver_str
        .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?
        .trim();

    if receiver_ident.is_empty() {
        return None;
    }

    Some(receiver_ident.to_string())
}

pub fn build_reflection_completions(
    state: &DocumentState,
    receiver_name: &str,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Meta Reflection properties (name, fields, methods, type, etc.)
    for (idx, meta) in META_PROPERTIES.iter().enumerate() {
        seen.insert(meta.name.to_string());
        let (insert_text, insert_text_format) = match meta.snippet {
            Some(s) => (Some(s.to_string()), Some(InsertTextFormat::SNIPPET)),
            None => (Some(meta.name.to_string()), None),
        };

        items.push(CompletionItem {
            label: meta.name.to_string(),
            kind: Some(meta.kind),
            detail: Some(format!("{}::{}: {}", receiver_name, meta.name, meta.detail)),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: meta.doc.to_string(),
            })),
            insert_text,
            insert_text_format,
            sort_text: Some(format!("0_{:02}_{}", idx, meta.name)),
            ..Default::default()
        });
    }

    // 2. Static members, Enum variants & constructors on the target receiver
    for sym in state.symbols() {
        if sym.name() == receiver_name {
            // If receiver is an Enum, add its variants
            if sym.kind() == varn_checker::SymbolKind::Enum {
                let members = state.members_of(sym);
                for m in members {
                    let m_name = m.name.to_string();
                    if seen.insert(m_name.clone()) {
                        items.push(CompletionItem {
                            label: m_name.clone(),
                            kind: Some(CompletionItemKind::ENUM_MEMBER),
                            detail: Some(format!("{}::{}", receiver_name, m_name)),
                            sort_text: Some(format!("1_{}", m_name)),
                            ..Default::default()
                        });
                    }
                }
            } else if sym.kind() == varn_checker::SymbolKind::Class {
                // If receiver is a Class, add static members
                let members = state.members_of(sym);
                for m in members {
                    let m_name = m.name.to_string();
                    if m.is_static && seen.insert(m_name.clone()) {
                        items.push(CompletionItem {
                            label: m_name.clone(),
                            kind: Some(if matches!(m.kind, varn_checker::ResolvedMemberKind::Method | varn_checker::ResolvedMemberKind::StaticMethod) {
                                CompletionItemKind::METHOD
                            } else {
                                CompletionItemKind::PROPERTY
                            }),
                            detail: Some(format!("static {}::{}: {}", receiver_name, m_name, m.ty)),
                            sort_text: Some(format!("1_{}", m_name)),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    items
}
