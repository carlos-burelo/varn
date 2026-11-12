use crate::document::DocumentState;
use rustc_hash::FxHashSet;
use tower_lsp_f::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use vn_core::TokenKind;
use vn_sem::semantic_info::{NestedTypeKind, ResolvedMemberKind, ResolvedMemberSummary};
use vn_sem::types::Type;

pub struct ReceiverInfo {
    pub ty: Type,

    pub is_instance: bool,
}

pub fn build_member_completions(
    state: &DocumentState,
    info: ReceiverInfo,
    use_snippets: bool,
    prefix: &str,
    cursor: u32,
) -> Vec<CompletionItem> {
    let (ty, is_instance) = unwrap_class_object(state, info);
    let viewer = enclosing_class(state, cursor);
    let mut members: Vec<_> = state
        .members_of_type(&ty)
        .into_iter()
        .filter(|m| m.kind != ResolvedMemberKind::Constructor)
        .filter(|m| m.is_static != is_instance)
        .filter(|m| is_accessible(m, viewer.as_deref()))
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    let mut seen = FxHashSet::default();
    members
        .iter()
        .filter(|m| seen.insert(m.name.clone()))
        .map(|m| summary_to_completion_item(state, m, use_snippets, prefix))
        .collect()
}

fn unwrap_class_object(state: &DocumentState, info: ReceiverInfo) -> (Type, bool) {
    let table = state.db.types.borrow();
    if info.ty.builtin_of(&table) == Some(vn_core::BuiltinType::Class) {
        return (info.ty.builtin_arg(0, &table), false);
    }
    (info.ty, info.is_instance)
}

fn enclosing_class(state: &DocumentState, cursor: u32) -> Option<String> {
    let tokens = &state.tokens;
    let mut open_braces: Vec<Option<String>> = Vec::new();
    let mut pending: Option<String> = None;
    for (i, tok) in tokens.iter().enumerate() {
        if tok.offset >= cursor {
            break;
        }
        if matches!(tok.kind, TokenKind::Class | TokenKind::Struct) {
            pending = tokens
                .get(i + 1)
                .filter(|next| next.kind == TokenKind::Identifier)
                .map(|next| state.lexeme(next).to_owned());
        } else if tok.kind == TokenKind::LBrace {
            open_braces.push(pending.take());
        } else if tok.kind == TokenKind::RBrace {
            open_braces.pop();
        }
    }
    open_braces.into_iter().rev().flatten().next()
}

fn is_accessible(member: &ResolvedMemberSummary, viewer: Option<&str>) -> bool {
    match member.visibility {
        Some(vn_core::ast::Visibility::Private) => {
            viewer.is_some_and(|class| member.owner.as_deref() == Some(class))
        }
        Some(vn_core::ast::Visibility::Protected) => viewer.is_some(),
        Some(vn_core::ast::Visibility::Public) | None => true,
    }
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
            NestedTypeKind::Class => CompletionItemKind::Class,
        },
    }
}
