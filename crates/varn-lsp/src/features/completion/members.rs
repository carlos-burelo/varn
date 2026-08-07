use crate::document::{DocumentState, MemberKind, MemberRecord};
use crate::util::kinds::member_to_completion_kind;
use tower_lsp::lsp_types::{CompletionItem, InsertTextFormat};
use varn_core::{IntrinsicType, TokenKind};

pub enum ReceiverInfo {
    Named {
        name: String,
        is_instance: bool,
        origin: Option<String>,
    },

    Anonymous(Vec<MemberRecord>),
}

pub fn build_member_completions(
    state: &DocumentState,
    info: ReceiverInfo,
    use_snippets: bool,
) -> Vec<CompletionItem> {
    match info {
        ReceiverInfo::Named {
            name,
            is_instance,
            origin,
        } => {
            let mut target_bind = None;
            if let Some(origin_mod) = origin.as_deref() {
                if origin_mod.starts_with("std:")
                    || origin_mod.starts_with("runtime:")
                    || origin_mod.starts_with("core:")
                {
                    target_bind =
                        varn_checker::module_resolver::resolve_stdlib_module_bind_ref(origin_mod);
                } else {
                    target_bind =
                        varn_checker::module_resolver::resolve_module_bind_ref(origin_mod);
                }
            }

            let mut seen = std::collections::HashSet::new();
            let mut items = Vec::new();

            if let Some(sym) = state.symbols.iter().find(|s| s.name == name) {
                for m in sym
                    .members
                    .iter()
                    .filter(|m| m.is_static != is_instance)
                    .filter(|m| m.kind != MemberKind::Constructor)
                {
                    if seen.insert(m.name.clone()) {
                        items.push(member_to_completion_item(m, use_snippets));
                    }
                }
            } else if let Some(tb) = &target_bind {
                let mapped = if let Some(class_info) = tb.get_class_entry(&name) {
                    crate::pipeline::symbols::map_members(&class_info.members, &[])
                } else if let Some(interface_info) = tb.get_interface_members_local(&name) {
                    crate::pipeline::symbols::map_members(interface_info, &[])
                } else if let Some(namespace_info) = tb.get_namespace_members_local(&name) {
                    crate::pipeline::symbols::map_members(namespace_info, &[])
                } else if let Some(enum_info) = tb.get_enum_members_local(&name) {
                    crate::pipeline::symbols::map_enum_members(enum_info, &[])
                } else {
                    Vec::new()
                };

                for m in mapped
                    .iter()
                    .filter(|m| m.is_static != is_instance)
                    .filter(|m| m.kind != MemberKind::Constructor)
                {
                    if seen.insert(m.name.clone()) {
                        items.push(member_to_completion_item(m, use_snippets));
                    }
                }
            }

            if is_instance {
                if let Some(exts) = state.db.extension_members.get(&name) {
                    for m in exts {
                        if seen.insert(m.name.clone()) {
                            items.push(member_to_completion_item(m, use_snippets));
                        }
                    }
                }
            }

            items
        }
        ReceiverInfo::Anonymous(members) => members
            .into_iter()
            .filter(|m| m.kind != MemberKind::Constructor)
            .map(|m| member_to_completion_item(&m, use_snippets))
            .collect(),
    }
}

fn member_to_completion_item(m: &MemberRecord, use_snippets: bool) -> CompletionItem {
    let (insert_text, insert_text_format) = match m.kind {
        MemberKind::Method | MemberKind::Function => {
            if use_snippets {
                (format!("{}($0)", m.name), Some(InsertTextFormat::SNIPPET))
            } else {
                (m.name.clone(), None)
            }
        }
        _ => (m.name.clone(), None),
    };
    let detail = match m.kind {
        MemberKind::Property | MemberKind::Variable | MemberKind::Getter => {
            Some(m.type_str.clone())
        }
        MemberKind::Setter => Some(format!("({})", m.params_str)),
        MemberKind::Constructor => Some(format!("({})", m.params_str)),
        MemberKind::Method | MemberKind::Function => {
            Some(format!("({}): {}", m.params_str, m.type_str))
        }
        MemberKind::Class => Some("class".to_owned()),
        MemberKind::Interface => Some("interface".to_owned()),
        MemberKind::Namespace => Some("namespace".to_owned()),
        MemberKind::Enum | MemberKind::EnumMember => Some("enum".to_owned()),
        MemberKind::Struct => Some("struct".to_owned()),
    };
    CompletionItem {
        label: m.name.clone(),
        kind: Some(member_to_completion_kind(m.kind)),
        detail,
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

        match &ty.0 {
            varn_core::TypeKind::Object(members) => {
                let recs = members.iter().filter_map(type_member_to_record).collect();
                return Some(ReceiverInfo::Anonymous(recs));
            }
            varn_core::TypeKind::Tuple(elems) => {
                let mut recs: Vec<MemberRecord> = elems
                    .iter()
                    .enumerate()
                    .map(|(idx, elem_ty)| MemberRecord {
                        name: idx.to_string(),
                        type_str: elem_ty.to_string(),
                        params_str: String::new(),
                        is_static: false,
                        is_optional: false,
                        kind: MemberKind::Property,
                        is_arrow: false,
                        is_async: false,
                        is_generator: false,
                        line: 0,
                        col: 0,
                        init_value: String::new(),
                        ty: elem_ty.clone(),
                        symbol_id: None,
                        members: Vec::new(),
                    })
                    .collect();
                recs.push(MemberRecord {
                    name: "length".to_string(),
                    type_str: "int".to_string(),
                    params_str: String::new(),
                    is_static: false,
                    is_optional: false,
                    kind: MemberKind::Property,
                    is_arrow: false,
                    is_async: false,
                    is_generator: false,
                    line: 0,
                    col: 0,
                    init_value: String::new(),
                    ty: varn_checker::Type(varn_core::TypeKind::Intrinsic(varn_core::TypeTag::Int), false),
                    symbol_id: None,
                    members: Vec::new(),
                });
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
                let mut is_instance = true;
                if let Some(sym) = state
                    .symbols
                    .iter()
                    .find(|s| s.name.as_str() == name.as_ref())
                {
                    if matches!(
                        sym.kind,
                        varn_checker::SymbolKind::Class
                            | varn_checker::SymbolKind::Namespace
                            | varn_checker::SymbolKind::Interface
                            | varn_checker::SymbolKind::Enum
                    ) {
                        is_instance = false;
                    }
                }
                return Some(ReceiverInfo::Named {
                    name: name.to_string(),
                    is_instance,
                    origin: origin.as_ref().map(|s| s.to_string()),
                });
            }
            _ => {}
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

fn type_member_to_record(m: &varn_checker::types::ObjectTypeMember) -> Option<MemberRecord> {
    use varn_checker::types::ObjectTypeMember::*;
    match m {
        Property { name, ty, .. } => Some(MemberRecord {
            name: name.to_string(),
            type_str: ty.to_string(),
            params_str: String::new(),
            is_static: false,
            is_optional: false,
            kind: MemberKind::Property,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            line: 0,
            col: 0,
            init_value: String::new(),
            ty: ty.clone(),
            symbol_id: None,
            members: Vec::new(),
        }),
        Method {
            name,
            params,
            return_type,
            ..
        } => {
            let params_str = params
                .iter()
                .map(|p| format!("{}: {}", p.name.as_deref().unwrap_or("arg"), p.ty))
                .collect::<Vec<_>>()
                .join(", ");
            Some(MemberRecord {
                name: name.to_string(),
                type_str: return_type.to_string(),
                params_str,
                is_static: false,
                is_optional: false,
                kind: MemberKind::Method,
                is_arrow: false,
                is_async: false,
                is_generator: false,
                line: 0,
                col: 0,
                init_value: String::new(),
                ty: varn_checker::Type(
                    varn_core::TypeKind::Fn(varn_checker::types::FunctionType {
                        params: params.clone(),
                        return_type: return_type.clone(),
                        is_arrow: false,
                        type_params: Vec::new(),
                    }),
                    false,
                ),
                symbol_id: None,
                members: Vec::new(),
            })
        }
        _ => None,
    }
}

fn dot_receiver_source_fallback(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<ReceiverInfo> {
    if col < 2 {
        return None;
    }
    let src_line = state.source.lines().nth(line as usize)?;
    let id_end = (col as usize)
        .saturating_sub(2)
        .min(src_line.len().saturating_sub(1));

    let bytes = src_line.as_bytes();
    if id_end >= bytes.len() {
        return None;
    }
    if !bytes[id_end].is_ascii_alphanumeric() && bytes[id_end] != b'_' {
        return None;
    }

    let mut start = id_end;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    let name = &src_line[start..=id_end];
    if name.is_empty() {
        return None;
    }

    let sym = state.symbols.iter().find(|s| s.name == name)?;
    match sym.kind {
        varn_checker::SymbolKind::Class
        | varn_checker::SymbolKind::Namespace
        | varn_checker::SymbolKind::Interface
        | varn_checker::SymbolKind::Enum => Some(ReceiverInfo::Named {
            name: sym.name.clone(),
            is_instance: false,
            origin: sym.origin.clone(),
        }),

        varn_checker::SymbolKind::Let
        | varn_checker::SymbolKind::Var
        | varn_checker::SymbolKind::Const
        | varn_checker::SymbolKind::Parameter => {
            if !sym.members.is_empty() {
                return Some(ReceiverInfo::Anonymous(sym.members.clone()));
            }
            if sym.type_str.is_empty() {
                return None;
            }
            if sym.type_str.ends_with("[]") {
                return Some(ReceiverInfo::Named {
                    name: IntrinsicType::Array.as_str().to_owned(),
                    is_instance: true,
                    origin: None,
                });
            }
            let class_name = sym.type_str.split('<').next()?.trim();
            state
                .symbols
                .iter()
                .find(|s| {
                    s.name == class_name
                        && !s.members.is_empty()
                        && matches!(
                            s.kind,
                            varn_checker::SymbolKind::Class
                                | varn_checker::SymbolKind::Interface
                                | varn_checker::SymbolKind::Namespace
                        )
                })
                .map(|s| ReceiverInfo::Named {
                    name: s.name.clone(),
                    is_instance: true,
                    origin: s.origin.clone(),
                })
        }

        _ => None,
    }
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
                let recs = members.iter().filter_map(type_member_to_record).collect();
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
