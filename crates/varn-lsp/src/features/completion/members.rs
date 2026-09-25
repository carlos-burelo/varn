//! Completion of the members a receiver offers: `x.|`, `Foo.|`, `{ | } = x`.
//!
//! Every receiver comes down to a type and whether the static or the
//! instance side is being read; the checker lists that type's members
//! (`get_members_of_type`: classes, generics substituted, primitives through
//! their core classes, structural objects, tuples, extensions).

use crate::document::{DocumentState, TokenRecord};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use varn_checker::{NestedTypeKind, ResolvedMemberKind, ResolvedMemberSummary, SymbolKind, Type};
use varn_core::{LangPrimitive, TokenKind};

/// What a member completion reads from.
pub struct ReceiverInfo {
    pub ty: Type,
    /// `x.` reads the instance side, `Foo.` the static one.
    pub is_instance: bool,
}

pub fn build_member_completions(
    state: &DocumentState,
    info: ReceiverInfo,
    use_snippets: bool,
) -> Vec<CompletionItem> {
    let mut seen = std::collections::HashSet::new();
    state
        .members_of_type(&info.ty)
        .iter()
        .filter(|m| m.kind != ResolvedMemberKind::Constructor)
        .filter(|m| m.is_static != info.is_instance)
        .filter(|m| seen.insert(m.name.clone()))
        .map(|m| summary_to_completion_item(state, m, use_snippets))
        .collect()
}

/// A completion item straight from the checker's summary of a member.
fn summary_to_completion_item(
    state: &DocumentState,
    m: &ResolvedMemberSummary,
    use_snippets: bool,
) -> CompletionItem {
    let is_method = matches!(
        m.kind,
        ResolvedMemberKind::Method
            | ResolvedMemberKind::StaticMethod
            | ResolvedMemberKind::ExtensionMethod
    );
    let (insert_text, insert_text_format) = if is_method && use_snippets {
        (format!("{}($0)", m.name), Some(InsertTextFormat::SNIPPET))
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
        ..Default::default()
    }
}

fn completion_kind(kind: ResolvedMemberKind) -> CompletionItemKind {
    match kind {
        ResolvedMemberKind::Method
        | ResolvedMemberKind::StaticMethod
        | ResolvedMemberKind::ExtensionMethod => CompletionItemKind::METHOD,
        ResolvedMemberKind::Property
        | ResolvedMemberKind::StaticProperty
        | ResolvedMemberKind::ExtensionProperty
        | ResolvedMemberKind::Getter
        | ResolvedMemberKind::Setter => CompletionItemKind::PROPERTY,
        ResolvedMemberKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
        ResolvedMemberKind::Constructor => CompletionItemKind::CONSTRUCTOR,
        ResolvedMemberKind::NestedType(k) => match k {
            NestedTypeKind::Interface => CompletionItemKind::INTERFACE,
            NestedTypeKind::Namespace => CompletionItemKind::MODULE,
            NestedTypeKind::Enum => CompletionItemKind::ENUM,
            NestedTypeKind::Class | NestedTypeKind::Struct => CompletionItemKind::CLASS,
        },
    }
}

/// Whether a symbol of `kind` names a type, so that a member access on it
/// reads the static side.
fn names_a_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Namespace
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::Struct
    )
}

pub fn dot_receiver(
    state: &DocumentState,
    line: u32,
    col: u32,
    trigger_char: Option<&str>,
) -> Option<ReceiverInfo> {
    let line_toks: Vec<_> = state.tokens.iter().filter(|t| t.line == line).collect();

    let Some(dot_idx) = line_toks
        .iter()
        .rposition(|t| t.kind == TokenKind::Dot && t.col < col)
    else {
        if trigger_char == Some(".") {
            return dot_receiver_source_fallback(state, line, col);
        }
        return None;
    };

    for t in line_toks.iter().skip(dot_idx + 1) {
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
        let is_instance = !info
            .symbol_id
            .filter(|s| *s < state.db.arena.len())
            .is_some_and(|sid| names_a_type(state.db.arena.get(sid).kind));
        return Some(ReceiverInfo {
            ty: state.db.non_null(&info.ty),
            is_instance,
        });
    }

    if let Some((sid, ty)) = state.db.resolve_at(&before.lexeme, before.offset) {
        if sid < state.db.arena.len() {
            let is_instance = !names_a_type(state.db.arena.get(sid).kind);
            return Some(ReceiverInfo { ty, is_instance });
        }
    }

    literal_receiver(state, before)
}

/// A literal's own primitive: `"a".|`, `1.|`.
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

/// The receiver of a `.` the parser could not make an expression of yet (the
/// member name is still missing): the name before it, resolved in scope.
fn dot_receiver_source_fallback(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> Option<ReceiverInfo> {
    let dot_tok = state
        .tokens
        .iter()
        .filter(|t| {
            t.line == line
                && t.col < col
                && (t.kind == TokenKind::Dot || t.kind == TokenKind::QuestionDot)
        })
        .max_by_key(|t| t.col)?;

    let receiver_tok = state
        .tokens
        .iter()
        .filter(|t| t.line == line && t.col < dot_tok.col)
        .max_by_key(|t| t.col)?;

    if receiver_tok.kind != TokenKind::Identifier {
        return None;
    }
    let (sid, ty) = state
        .db
        .resolve_at(&receiver_tok.lexeme, receiver_tok.offset)?;
    if sid < state.db.arena.len() {
        let sym = state.db.arena.get(sid);
        if names_a_type(sym.kind) {
            return Some(ReceiverInfo {
                ty: state.db.named_type(state.name(sym.name)),
                is_instance: false,
            });
        }
    }
    Some(ReceiverInfo {
        ty: state.db.non_null(&ty),
        is_instance: true,
    })
}

/// The value a destructuring pattern `{ | } = x` reads fields from.
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
        ty: info.ty,
        is_instance: true,
    })
}
