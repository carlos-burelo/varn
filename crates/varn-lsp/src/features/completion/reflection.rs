use rustc_hash::FxHashSet;
use tower_lsp_f::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, InsertTextFormat, MarkupContent, MarkupKind,
};
use varn_checker::SymbolKind;
use varn_core::TokenKind;

use super::members::member_group;
use super::scope::prefix_boost;
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
        kind: CompletionItemKind::Property,
        detail: "Reflection: type / class name (str)",
        doc: "Returns the name of the class or type as a `str`.",
        snippet: None,
    },
    MetaProperty {
        name: "fields",
        kind: CompletionItemKind::Property,
        detail: "Reflection: field names (str[])",
        doc: "Returns an array of field names declared on this type:\n```varn\nSampleUser::fields // [\"name\", \"age\", \"role\"]\n```",
        snippet: None,
    },
    MetaProperty {
        name: "methods",
        kind: CompletionItemKind::Property,
        detail: "Reflection: method names (str[])",
        doc: "Returns an array of method names declared on this type:\n```varn\nSampleUser::methods // [\"greet\", \"save\"]\n```",
        snippet: None,
    },
    MetaProperty {
        name: "type",
        kind: CompletionItemKind::Property,
        detail: "Reflection: type tag (str)",
        doc: "Returns the intrinsic or user type tag string.",
        snippet: None,
    },
    MetaProperty {
        name: "class",
        kind: CompletionItemKind::Property,
        detail: "Reflection: class constructor reference",
        doc: "Returns the runtime class constructor object.",
        snippet: None,
    },
    MetaProperty {
        name: "keys",
        kind: CompletionItemKind::Method,
        detail: "Reflection: keys() -> str[]",
        doc: "Returns the reflection keys of this type.",
        snippet: Some("keys()$0"),
    },
    MetaProperty {
        name: "values",
        kind: CompletionItemKind::Method,
        detail: "Reflection: values() -> dynamic[]",
        doc: "Returns the property values of this type.",
        snippet: Some("values()$0"),
    },
    MetaProperty {
        name: "entries",
        kind: CompletionItemKind::Method,
        detail: "Reflection: entries() -> [str, dynamic][]",
        doc: "Returns key-value pairs of this type.",
        snippet: Some("entries()$0"),
    },
    MetaProperty {
        name: "hasOwn",
        kind: CompletionItemKind::Method,
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
) -> Option<(String, u32)> {
    let line_toks: Vec<_> = state.tokens.iter().filter(|t| t.line == line).collect();
    let cc_idx = line_toks
        .iter()
        .rposition(|t| t.kind == TokenKind::ColonColon && t.col < col)?;

    for t in line_toks.iter().skip(cc_idx + 1) {
        if t.col >= col {
            break;
        }
        if t.kind != TokenKind::Identifier && !t.kind.can_be_identifier() {
            return None;
        }
    }

    let mut recv_idx = cc_idx.checked_sub(1)?;
    if line_toks[recv_idx].kind == TokenKind::RAngle {
        let mut angle_balance = 1;
        let mut found = None;
        for i in (0..recv_idx).rev() {
            if line_toks[i].kind == TokenKind::RAngle {
                angle_balance += 1;
            } else if line_toks[i].kind == TokenKind::LAngle {
                angle_balance -= 1;
                if angle_balance == 0 {
                    found = i.checked_sub(1);
                    break;
                }
            }
        }
        recv_idx = found?;
    }

    let receiver = line_toks[recv_idx];
    if receiver.kind != TokenKind::Identifier && !receiver.kind.can_be_identifier() {
        return None;
    }
    Some((state.lexeme(receiver).to_string(), receiver.offset))
}

pub fn build_reflection_completions(
    state: &DocumentState,
    receiver_name: &str,
    receiver_offset: u32,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = FxHashSet::default();

    for (idx, meta) in META_PROPERTIES.iter().enumerate() {
        seen.insert(meta.name.to_string());
        let (insert_text, insert_text_format) = match meta.snippet {
            Some(s) => (Some(s.to_string()), Some(InsertTextFormat::Snippet)),
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
            sort_text: Some(format!(
                "0_{}_{:02}_{}",
                prefix_boost(meta.name, prefix),
                idx,
                meta.name
            )),
            ..Default::default()
        });
    }

    if let Some((sid, _)) = state.db.resolve_at(receiver_name, receiver_offset) {
        if sid < state.db.bind.arena.len() {
            let sym = state.symbol(sid);
            if sym.kind() == SymbolKind::Enum {
                let members = state.members_of(sym);
                for m in members {
                    let m_name = m.name.to_string();
                    if seen.insert(m_name.clone()) {
                        items.push(CompletionItem {
                            label: m_name.clone(),
                            kind: Some(CompletionItemKind::EnumMember),
                            detail: Some(format!("{}::{}", receiver_name, m_name)),
                            sort_text: Some(format!(
                                "1_{}_{}_{}",
                                member_group(m.kind),
                                prefix_boost(&m_name, prefix),
                                m_name
                            )),
                            ..Default::default()
                        });
                    }
                }
            } else if sym.kind() == SymbolKind::Class || sym.kind() == SymbolKind::Interface {
                let members = state.members_of(sym);
                for m in members {
                    let m_name = m.name.to_string();
                    if m.is_static && seen.insert(m_name.clone()) {
                        items.push(CompletionItem {
                            label: m_name.clone(),
                            kind: Some(
                                if matches!(
                                    m.kind,
                                    varn_checker::ResolvedMemberKind::Method
                                        | varn_checker::ResolvedMemberKind::StaticMethod
                                ) {
                                    CompletionItemKind::Method
                                } else {
                                    CompletionItemKind::Property
                                },
                            ),
                            detail: Some(format!(
                                "static {}::{}: {}",
                                receiver_name,
                                m_name,
                                state.ty_text(&m.ty)
                            )),
                            sort_text: Some(format!(
                                "1_{}_{}_{}",
                                member_group(m.kind),
                                prefix_boost(&m_name, prefix),
                                m_name
                            )),
                            ..Default::default()
                        });
                    }
                }
            } else if sym.kind() == SymbolKind::Namespace {
                let members = state.members_of(sym);
                for m in members {
                    let m_name = m.name.to_string();
                    if seen.insert(m_name.clone()) {
                        items.push(CompletionItem {
                            label: m_name.clone(),
                            kind: Some(
                                if matches!(
                                    m.kind,
                                    varn_checker::ResolvedMemberKind::Method
                                        | varn_checker::ResolvedMemberKind::StaticMethod
                                ) {
                                    CompletionItemKind::Method
                                } else {
                                    CompletionItemKind::Property
                                },
                            ),
                            detail: Some(format!(
                                "{}::{}: {}",
                                receiver_name,
                                m_name,
                                state.ty_text(&m.ty)
                            )),
                            sort_text: Some(format!(
                                "1_{}_{}_{}",
                                member_group(m.kind),
                                prefix_boost(&m_name, prefix),
                                m_name
                            )),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    items
}
