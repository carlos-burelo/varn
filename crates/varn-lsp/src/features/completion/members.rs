use crate::document::DocumentState;
use tower_lsp::lsp_types::{CompletionItem, InsertTextFormat};
use varn_core::{IntrinsicType, TokenKind};

pub enum ReceiverInfo {
    Named {
        name: String,
        is_instance: bool,
        origin: Option<String>,
    },

    Typed {
        ty: varn_checker::Type,
        is_instance: bool,
    },

    Anonymous(Vec<varn_checker::ResolvedMemberSummary>),
}

pub fn build_member_completions(
    state: &DocumentState,
    info: ReceiverInfo,
    use_snippets: bool,
) -> Vec<CompletionItem> {
    match info {
        ReceiverInfo::Typed { ty, is_instance } => {
            let members = crate::workspace::resolver::with_resolver(|r| {
                varn_checker::get_members_of_type(r, &ty, &state.db.bind)
            });
            let mut seen = std::collections::HashSet::new();
            let mut items = Vec::new();

            for m in members {
                if m.is_static != !is_instance {
                    continue;
                }
                if !seen.insert(m.name.to_string()) {
                    continue;
                }
                let is_method = matches!(
                    m.kind,
                    varn_checker::ResolvedMemberKind::Method
                        | varn_checker::ResolvedMemberKind::StaticMethod
                        | varn_checker::ResolvedMemberKind::ExtensionMethod
                );
                let (insert_text, insert_text_format) = if is_method {
                    if use_snippets {
                        (format!("{}($0)", m.name), Some(InsertTextFormat::SNIPPET))
                    } else {
                        (m.name.to_string(), None)
                    }
                } else {
                    (m.name.to_string(), None)
                };

                let detail = match &m.ty.0 {
                    varn_core::TypeKind::Fn(ft) => {
                        let params = ft
                            .params
                            .iter()
                            .map(|p| {
                                let n = p.name.as_deref().unwrap_or("arg");
                                format!("{}: {}", n, p.ty)
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("({}): {}", params, ft.return_type)
                    }
                    _ => m.ty.to_string(),
                };

                let kind = match m.kind {
                    varn_checker::ResolvedMemberKind::Method
                    | varn_checker::ResolvedMemberKind::StaticMethod
                    | varn_checker::ResolvedMemberKind::ExtensionMethod => {
                        tower_lsp::lsp_types::CompletionItemKind::METHOD
                    }
                    varn_checker::ResolvedMemberKind::Property
                    | varn_checker::ResolvedMemberKind::StaticProperty
                    | varn_checker::ResolvedMemberKind::ExtensionProperty => {
                        tower_lsp::lsp_types::CompletionItemKind::PROPERTY
                    }
                    varn_checker::ResolvedMemberKind::Getter => {
                        tower_lsp::lsp_types::CompletionItemKind::PROPERTY
                    }
                    varn_checker::ResolvedMemberKind::Setter => {
                        tower_lsp::lsp_types::CompletionItemKind::PROPERTY
                    }
                    varn_checker::ResolvedMemberKind::EnumMember => {
                        tower_lsp::lsp_types::CompletionItemKind::ENUM_MEMBER
                    }
                    varn_checker::ResolvedMemberKind::Constructor => {
                        tower_lsp::lsp_types::CompletionItemKind::CONSTRUCTOR
                    }
                    varn_checker::ResolvedMemberKind::NestedType(k) => match k {
                        varn_checker::NestedTypeKind::Interface => {
                            tower_lsp::lsp_types::CompletionItemKind::INTERFACE
                        }
                        varn_checker::NestedTypeKind::Namespace => {
                            tower_lsp::lsp_types::CompletionItemKind::MODULE
                        }
                        varn_checker::NestedTypeKind::Enum => {
                            tower_lsp::lsp_types::CompletionItemKind::ENUM
                        }
                        _ => tower_lsp::lsp_types::CompletionItemKind::CLASS,
                    },
                };

                items.push(CompletionItem {
                    label: m.name.to_string(),
                    kind: Some(kind),
                    detail: Some(detail),
                    insert_text: Some(insert_text),
                    insert_text_format,
                    ..Default::default()
                });
            }

            // Extensions are already in `members` above: the checker returns
            // them from `get_members_of_type`, which is where they belong.
            items
        }
        ReceiverInfo::Named {
            name, is_instance, ..
        } => {
            // One question, one answer. This used to try three sources in turn
            // — the mirrored member table, then a foreign module's bind through
            // four `get_*_local` fallbacks, then extensions from a separate map
            // — because no single one of them was complete. Asking the checker
            // about the type is complete by construction: it substitutes
            // generics, follows origins, and includes extensions.
            let ty = varn_checker::Type::named(name.clone());
            let mut seen = std::collections::HashSet::new();
            state
                .members_of_type(&ty)
                .iter()
                .filter(|m| m.is_static != is_instance)
                .filter(|m| seen.insert(m.name.to_string()))
                .map(|m| summary_to_completion_item(m, use_snippets))
                .collect()
        }
        ReceiverInfo::Anonymous(members) => members
            .iter()
            .filter(|m| m.kind != varn_checker::ResolvedMemberKind::Constructor)
            .map(|m| summary_to_completion_item(m, use_snippets))
            .collect(),
    }
}

/// A completion item straight from the checker's summary of a member.
fn summary_to_completion_item(
    m: &varn_checker::ResolvedMemberSummary,
    use_snippets: bool,
) -> CompletionItem {
    let is_method = matches!(
        m.kind,
        varn_checker::ResolvedMemberKind::Method
            | varn_checker::ResolvedMemberKind::StaticMethod
            | varn_checker::ResolvedMemberKind::ExtensionMethod
    );
    let (insert_text, insert_text_format) = if is_method && use_snippets {
        (format!("{}($0)", m.name), Some(InsertTextFormat::SNIPPET))
    } else {
        (m.name.to_string(), None)
    };
    let detail = match &m.ty.0 {
        varn_core::TypeKind::Fn(ft) => {
            let params = ft
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name.as_deref().unwrap_or("arg"), p.ty))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({}): {}", params, ft.return_type)
        }
        _ => m.ty.to_string(),
    };
    CompletionItem {
        label: m.name.to_string(),
        kind: Some(if is_method {
            tower_lsp::lsp_types::CompletionItemKind::METHOD
        } else {
            tower_lsp::lsp_types::CompletionItemKind::PROPERTY
        }),
        detail: Some(detail),
        insert_text: Some(insert_text),
        insert_text_format,
        ..Default::default()
    }
}

pub fn dot_receiver(
    state: &DocumentState,
    line: u32,
    col: u32,
    trigger_char: Option<&str>,
) -> Option<ReceiverInfo> {
    let line_toks: Vec<_> = state.tokens.iter().filter(|t| t.line == line).collect();

    let dot_idx = line_toks
        .iter()
        .rposition(|t| t.kind == TokenKind::Dot && t.col < col);

    if dot_idx.is_none() {
        if trigger_char == Some(".") {
            return dot_receiver_source_fallback(state, line, col);
        }
        return None;
    }
    let dot_idx = dot_idx?;

    for i in (dot_idx + 1)..line_toks.len() {
        let t = line_toks[i];
        if t.col >= col {
            break;
        }
        if t.kind != TokenKind::Identifier && !t.kind.can_be_identifier() {
            return None;
        }
    }

    if dot_idx == 0 {
        return None;
    }

    let before = line_toks[dot_idx - 1];

    if let Some(info) = state.expr_info_at_token(before) {
        let mut ty = info.ty.clone();
        if ty.is_nullable() {
            ty = ty.non_nullified();
        }

        let mut is_instance = true;
        if let Some(sid) = info.symbol_id.filter(|s| *s < state.db.arena.len()) {
            let sym = state.db.arena.get(sid);
            if matches!(
                sym.kind,
                varn_checker::SymbolKind::Class
                    | varn_checker::SymbolKind::Namespace
                    | varn_checker::SymbolKind::Interface
                    | varn_checker::SymbolKind::Enum
                    | varn_checker::SymbolKind::Struct
            ) {
                is_instance = false;
            }
        }

        return Some(ReceiverInfo::Typed { ty, is_instance });
    }

    if let Some((sid, ty)) = state.db.resolve_at(&before.lexeme, before.offset) {
        if sid < state.db.arena.len() {
            let sym = state.db.arena.get(sid);
            let is_instance = !matches!(
                sym.kind,
                varn_checker::SymbolKind::Class
                    | varn_checker::SymbolKind::Namespace
                    | varn_checker::SymbolKind::Interface
                    | varn_checker::SymbolKind::Enum
                    | varn_checker::SymbolKind::Struct
            );
            return Some(ReceiverInfo::Typed { ty, is_instance });
        }
    }

    let literal_type: Option<&str> = match before.kind {
        TokenKind::Str => Some(IntrinsicType::Str.as_str()),
        TokenKind::IntegerLiteral
        | TokenKind::HexLiteral
        | TokenKind::BinaryLiteral
        | TokenKind::OctalLiteral => Some(IntrinsicType::Int.as_str()),
        TokenKind::FloatLiteral => Some(IntrinsicType::Float.as_str()),
        TokenKind::DecimalLiteral => Some(IntrinsicType::Decimal.as_str()),
        TokenKind::BigIntLiteral => Some(IntrinsicType::BigInt.as_str()),
        TokenKind::True | TokenKind::False => Some(IntrinsicType::Bool.as_str()),
        TokenKind::Char => Some(IntrinsicType::Char.as_str()),
        _ => None,
    };
    if let Some(prim) = literal_type {
        return Some(ReceiverInfo::Named {
            name: prim.to_owned(),
            is_instance: true,
            origin: None,
        });
    }

    None
}

/// A member of a structural object type, as a summary.
///
/// Anonymous objects declare no class, so there is no `ClassMemberInfo` to read;
/// the type itself is the declaration.
fn object_member_summary(
    m: &varn_checker::types::ObjectTypeMember,
) -> Option<varn_checker::ResolvedMemberSummary> {
    use varn_checker::types::ObjectTypeMember::*;
    let (name, ty, kind) = match m {
        Property { name, ty, .. } => (
            name.clone(),
            ty.clone(),
            varn_checker::ResolvedMemberKind::Property,
        ),
        Method {
            name,
            params,
            return_type,
            ..
        } => (
            name.clone(),
            varn_checker::Type(
                varn_core::TypeKind::Fn(varn_checker::types::FunctionType {
                    params: params.clone(),
                    return_type: return_type.clone(),
                    is_arrow: false,
                    type_params: Vec::new(),
                }),
                false,
            ),
            varn_checker::ResolvedMemberKind::Method,
        ),
        _ => return None,
    };
    Some(varn_checker::ResolvedMemberSummary {
        name,
        ty,
        kind,
        is_static: false,
        optional: false,
        readonly: false,
        def_line: None,
        def_col: 0,
        is_async: false,
        is_generator: false,
    })
}

fn dot_receiver_source_fallback(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<ReceiverInfo> {
    // Find the receiver token before the dot at (line, col)
    let dot_tok = state
        .tokens
        .iter()
        .filter(|t| t.line == line && t.col < col && (t.kind == TokenKind::Dot || t.kind == TokenKind::QuestionDot))
        .max_by_key(|t| t.col)?;

    let receiver_tok = state
        .tokens
        .iter()
        .filter(|t| t.line == line && t.col < dot_tok.col)
        .max_by_key(|t| t.col)?;

    if receiver_tok.kind == TokenKind::Identifier {
        if let Some((sid, ty)) = state.db.resolve_at(&receiver_tok.lexeme, receiver_tok.offset) {
            if sid < state.db.arena.len() {
                let sym = state.db.arena.get(sid);
                if matches!(
                    sym.kind,
                    varn_checker::SymbolKind::Class
                        | varn_checker::SymbolKind::Namespace
                        | varn_checker::SymbolKind::Interface
                        | varn_checker::SymbolKind::Enum
                        | varn_checker::SymbolKind::Struct
                ) {
                    return Some(ReceiverInfo::Named {
                        name: sym.name.to_string(),
                        is_instance: false,
                        origin: sym.origin_module.as_deref().map(str::to_string),
                    });
                }
            }

            match &ty.0 {
                varn_core::TypeKind::Object(members) => {
                    let recs = members.iter().filter_map(object_member_summary).collect();
                    return Some(ReceiverInfo::Anonymous(recs));
                }
                varn_core::TypeKind::Array(_) => {
                    return Some(ReceiverInfo::Named {
                        name: IntrinsicType::Array.as_str().to_owned(),
                        is_instance: true,
                        origin: None,
                    });
                }
                varn_core::TypeKind::Intrinsic(tag) => {
                    return Some(ReceiverInfo::Named {
                        name: tag.name().to_owned(),
                        is_instance: true,
                        origin: None,
                    });
                }
                varn_core::TypeKind::Named(name, origin)
                | varn_core::TypeKind::Generic(name, _, origin) => {
                    return Some(ReceiverInfo::Named {
                        name: name.to_string(),
                        is_instance: true,
                        origin: origin.as_ref().map(|s| s.to_string()),
                    });
                }
                _ => {}
            }
        }
    }

    None
}
pub fn pattern_receiver(state: &DocumentState, line: u32, col: u32) -> Option<ReceiverInfo> {
    let line_toks: Vec<_> = state.tokens.iter().filter(|t| t.line == line).collect();

    let _brace_open_idx = line_toks
        .iter()
        .rposition(|t| t.kind == TokenKind::LBrace && t.col < col)?;

    let eq_idx = line_toks
        .iter()
        .position(|t| t.kind == TokenKind::Eq && t.col >= col)?;

    let rhs_idx = eq_idx + 1;
    if rhs_idx >= line_toks.len() {
        return None;
    }
    let rhs_tok = line_toks[rhs_idx];

    if let Some(info) = state.expr_info_at_token(rhs_tok) {
        match &info.ty.0 {
            varn_core::TypeKind::Object(members) => {
                let recs = members.iter().filter_map(object_member_summary).collect();
                return Some(ReceiverInfo::Anonymous(recs));
            }
            varn_core::TypeKind::Intrinsic(tag) => {
                if tag.is_primitive() || tag == &varn_core::TypeTag::Array {
                    return Some(ReceiverInfo::Named {
                        name: tag.name().to_owned(),
                        is_instance: true,
                        origin: None,
                    });
                }
            }
            varn_core::TypeKind::Named(name, origin)
            | varn_core::TypeKind::Generic(name, _, origin) => {
                return Some(ReceiverInfo::Named {
                    name: name.to_string(),
                    is_instance: true,
                    origin: origin.as_ref().map(|s| s.to_string()),
                });
            }
            _ => {}
        }
    }

    None
}
