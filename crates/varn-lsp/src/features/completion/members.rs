use crate::document::DocumentState;
use rustc_hash::FxHashSet;
use tower_lsp_f::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use varn_sem::semantic_info::{NestedTypeKind, ResolvedMemberKind, ResolvedMemberSummary};
use varn_sem::types::Type;

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
    let mut seen = FxHashSet::default();
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
