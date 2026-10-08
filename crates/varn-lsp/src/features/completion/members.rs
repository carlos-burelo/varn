use crate::document::{DocumentState, TokenRecord};
use tower_lsp_f::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use varn_checker::{NestedTypeKind, ResolvedMemberKind, ResolvedMemberSummary, SymbolKind, Type};
use varn_core::{LangPrimitive, TokenKind};

pub struct ReceiverInfo {
    pub ty: Type,

    pub is_instance: bool,
}

pub fn build_member_completions(
    state: &DocumentState,
    info: ReceiverInfo,
    use_snippets: bool,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut members: Vec<_> = state
        .members_of_type(&info.ty)
        .into_iter()
        .filter(|m| m.kind != ResolvedMemberKind::Constructor)
        .filter(|m| m.is_static != info.is_instance)
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    let mut seen = std::collections::HashSet::new();
    members
        .iter()
        .filter(|m| seen.insert(m.name.clone()))
        .map(|m| summary_to_completion_item(state, m, use_snippets, prefix))
        .collect()
}

pub fn member_group(kind: ResolvedMemberKind) -> u8 {
    match kind {
        ResolvedMemberKind::Method
        | ResolvedMemberKind::StaticMethod
        | ResolvedMemberKind::ExtensionMethod => 0,
        ResolvedMemberKind::Property
        | ResolvedMemberKind::StaticProperty
        | ResolvedMemberKind::ExtensionProperty
        | ResolvedMemberKind::Getter
        | ResolvedMemberKind::Setter
        | ResolvedMemberKind::EnumMember => 1,
        ResolvedMemberKind::Constructor | ResolvedMemberKind::NestedType(_) => 2,
    }
}

fn summary_to_completion_item(
    state: &DocumentState,
    m: &ResolvedMemberSummary,
    use_snippets: bool,
    prefix: &str,
) -> CompletionItem {
    let is_method = matches!(
        m.kind,
        ResolvedMemberKind::Method
            | ResolvedMemberKind::StaticMethod
            | ResolvedMemberKind::ExtensionMethod
    );
    let (insert_text, insert_text_format) = if is_method && use_snippets {
        (format!("{}($0)", m.name), Some(InsertTextFormat::Snippet))
    } else {
        (m.name.to_string(), None)
    };
    let detail = match state.db.fn_shape(&m.ty) {
        Some(ft) => {
            let params = ft
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}",
                        p.name.as_deref().unwrap_or("arg"),
                        state.db.id_text(p.ty)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("({params}): {}", state.db.id_text(ft.return_type))
        }
        None => state.ty_text(&m.ty),
    };
    CompletionItem {
        label: m.name.to_string(),
        kind: Some(completion_kind(m.kind)),
        detail: Some(detail),
        insert_text: Some(insert_text),
        insert_text_format,
        sort_text: Some(format!(
            "{}_{}_{}",
            member_group(m.kind),
            super::scope::prefix_boost(&m.name, prefix),
            m.name
        )),
        ..Default::default()
    }
}

fn completion_kind(kind: ResolvedMemberKind) -> CompletionItemKind {
    match kind {
        ResolvedMemberKind::Method
        | ResolvedMemberKind::StaticMethod
        | ResolvedMemberKind::ExtensionMethod => CompletionItemKind::Method,
        ResolvedMemberKind::Property
        | ResolvedMemberKind::StaticProperty
        | ResolvedMemberKind::ExtensionProperty
        | ResolvedMemberKind::Getter
        | ResolvedMemberKind::Setter => CompletionItemKind::Property,
        ResolvedMemberKind::EnumMember => CompletionItemKind::EnumMember,
        ResolvedMemberKind::Constructor => CompletionItemKind::Constructor,
        ResolvedMemberKind::NestedType(k) => match k {
            NestedTypeKind::Interface => CompletionItemKind::Interface,
            NestedTypeKind::Namespace => CompletionItemKind::Module,
            NestedTypeKind::Enum => CompletionItemKind::Enum,
            NestedTypeKind::Class | NestedTypeKind::Struct => CompletionItemKind::Class,
        },
    }
}

fn names_a_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum | SymbolKind::Struct
    )
}

pub struct DotVerdict {
    pub info: Option<ReceiverInfo>,
    pub stage: &'static str,
    pub receiver: String,
}

fn miss(stage: &'static str, receiver: String) -> DotVerdict {
    DotVerdict {
        info: None,
        stage,
        receiver,
    }
}

fn hit(
    state: &DocumentState,
    stage: &'static str,
    tok: &TokenRecord,
    ty: Type,
    is_instance: bool,
) -> DotVerdict {
    DotVerdict {
        info: Some(ReceiverInfo {
            ty: state.db.non_null(&ty),
            is_instance,
        }),
        stage,
        receiver: state.lexeme(tok).to_owned(),
    }
}

pub fn dot_receiver(
    state: &DocumentState,
    line: u32,
    col: u32,
    trigger_char: Option<&str>,
) -> DotVerdict {
    let cursor_offset = state.offset_at_line_col(line, col);

    let Some(dot_pos) = state.tokens.iter().rposition(|t| {
        (t.kind == TokenKind::Dot || t.kind == TokenKind::QuestionDot) && t.offset < cursor_offset
    }) else {
        if trigger_char == Some(".") {
            return match dot_receiver_source_fallback(state, line, col, cursor_offset) {
                Some(info) => DotVerdict {
                    info: Some(info),
                    stage: "trigger-fallback",
                    receiver: String::new(),
                },
                None => miss("fallback-miss", String::new()),
            };
        }
        return miss("no-dot", String::new());
    };

    for t in state.tokens.iter().skip(dot_pos + 1) {
        if t.offset >= cursor_offset {
            break;
        }
        if t.kind != TokenKind::Identifier && !t.kind.can_be_identifier() {
            return miss("garbage", state.lexeme(t).to_owned());
        }
    }

    if dot_pos == 0 {
        return miss("lone-dot", String::new());
    }

    let after: Vec<_> = state
        .tokens
        .iter()
        .skip(dot_pos + 1)
        .filter(|t| t.offset < cursor_offset)
        .collect();
    if after.len() == 1 {
        if let Some(res) = state.db.member_resolutions.get(&after[0].offset) {
            let before = &state.tokens[dot_pos - 1];
            return hit(
                state,
                "partial",
                before,
                res.receiver_ty,
                !receiver_is_type_name(state, before),
            );
        }
    }

    let before = &state.tokens[dot_pos - 1];

    if let Some(res) = state.db.member_resolutions.get(&before.offset) {
        let is_instance = match res.member_kind {
            ResolvedMemberKind::NestedType(kind) => kind == NestedTypeKind::Namespace,
            _ => true,
        };
        return hit(state, "chained", before, res.member_ty, is_instance);
    }

    if let Some(info) = state.expr_info_at_token(before) {
        return hit(
            state,
            "expr-info",
            before,
            info.ty,
            !receiver_is_type_name(state, before),
        );
    }

    if let Some((_, ty)) = state.db.resolve_at(state.lexeme(before), before.offset) {
        return hit(
            state,
            "scope",
            before,
            ty,
            !receiver_is_type_name(state, before),
        );
    }

    if before.kind == TokenKind::Identifier || before.kind.can_be_identifier() {
        let ty = state.db.named_type(state.lexeme(before));
        if !state.members_of_type(&ty).is_empty() {
            return hit(state, "typename", before, ty, false);
        }
    }

    if let Some(entry) = state.expr_entry_at_offset(before.offset) {
        let is_instance = !entry
            .symbol_id
            .filter(|s| *s < state.db.bind.arena.len())
            .is_some_and(|sid| names_a_type(state.db.bind.arena.get(sid).kind));
        return hit(state, "spatial", before, entry.ty, is_instance);
    }

    match literal_receiver(state, before) {
        Some(info) => DotVerdict {
            info: Some(info),
            stage: "literal",
            receiver: state.lexeme(before).to_owned(),
        },
        None => miss("unknown-recv", state.lexeme(before).to_owned()),
    }
}

fn receiver_is_type_name(state: &DocumentState, tok: &TokenRecord) -> bool {
    if let Some(info) = state.expr_info_at_token(tok) {
        if let Some(sid) = info.symbol_id {
            if sid < state.db.bind.arena.len() {
                return names_a_type(state.db.bind.arena.get(sid).kind);
            }
        }
    }
    if let Some((sid, _)) = state.db.resolve_at(state.lexeme(tok), tok.offset) {
        if sid < state.db.bind.arena.len() {
            return names_a_type(state.db.bind.arena.get(sid).kind);
        }
    }
    false
}

fn literal_receiver(state: &DocumentState, tok: &TokenRecord) -> Option<ReceiverInfo> {
    let p = match tok.kind {
        TokenKind::Str => LangPrimitive::Str,
        TokenKind::IntegerLiteral
        | TokenKind::HexLiteral
        | TokenKind::BinaryLiteral
        | TokenKind::OctalLiteral => LangPrimitive::Int,
        TokenKind::FloatLiteral => LangPrimitive::Float,
        TokenKind::DecimalLiteral => LangPrimitive::Decimal,
        TokenKind::BigIntLiteral => LangPrimitive::BigInt,
        TokenKind::True | TokenKind::False => LangPrimitive::Bool,
        TokenKind::Char => LangPrimitive::Char,
        _ => return None,
    };
    Some(ReceiverInfo {
        ty: state.db.primitive(p),
        is_instance: true,
    })
}

fn dot_receiver_source_fallback(
    state: &DocumentState,
    _line: u32,
    _col: u32,
    cursor_offset: u32,
) -> Option<ReceiverInfo> {
    let receiver_tok = state
        .tokens
        .iter()
        .filter(|t| t.offset < cursor_offset)
        .max_by_key(|t| (t.offset, t.col))?;

    if receiver_tok.kind == TokenKind::RParen || receiver_tok.kind == TokenKind::RBracket {
        let entry = state.expr_entry_at_offset(receiver_tok.offset)?;
        let is_instance = !entry
            .symbol_id
            .filter(|s| *s < state.db.bind.arena.len())
            .is_some_and(|sid| names_a_type(state.db.bind.arena.get(sid).kind));
        return Some(ReceiverInfo {
            ty: state.db.non_null(&entry.ty),
            is_instance,
        });
    }

    if receiver_tok.kind == TokenKind::Identifier || receiver_tok.kind.can_be_identifier() {
        if let Some((sid, ty)) = state
            .db
            .resolve_at(state.lexeme(receiver_tok), receiver_tok.offset)
        {
            if sid < state.db.bind.arena.len() {
                let is_instance = !names_a_type(state.db.bind.arena.get(sid).kind);
                return Some(ReceiverInfo {
                    ty: state.db.non_null(&ty),
                    is_instance,
                });
            }
        }
        let ty = state.db.named_type(state.lexeme(receiver_tok));
        if !state.members_of_type(&ty).is_empty() {
            return Some(ReceiverInfo {
                ty: state.db.non_null(&ty),
                is_instance: false,
            });
        }
        return None;
    }

    literal_receiver(state, receiver_tok)
}

pub fn pattern_receiver(state: &DocumentState, line: u32, col: u32) -> Option<ReceiverInfo> {
    let line_toks: Vec<_> = state.tokens.iter().filter(|t| t.line == line).collect();

    line_toks
        .iter()
        .rposition(|t| t.kind == TokenKind::LBrace && t.col < col)?;

    let eq_idx = line_toks
        .iter()
        .position(|t| t.kind == TokenKind::Eq && t.col >= col)?;

    let rhs_tok = line_toks.get(eq_idx + 1)?;
    let info = state.expr_info_at_token(rhs_tok)?;
    Some(ReceiverInfo {
        ty: state.db.non_null(&info.ty),
        is_instance: true,
    })
}
