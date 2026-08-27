mod autoimport;
mod calls;
mod imports;
mod keywords;
pub mod members;
pub mod postfix;
pub mod reflection;
mod scope;

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionResponse, Documentation,
    InsertTextFormat, MarkupContent, MarkupKind,
};

use crate::document::{
    import_path_at, named_import_module_at, named_imported_names_at, DocumentState,
};
use crate::index::ProjectIndex;

pub use imports::{
    build_import_completions, build_module_export_completions, resolve_relative_module_debug,
};
pub use members::build_member_completions;
pub use postfix::build_postfix_completions;
pub use reflection::build_reflection_completions;

pub fn build_completion_response(
    state: &DocumentState,
    line: u32,
    col: u32,
    trigger_char: Option<&str>,
    trigger_kind: String,
    index: Option<&ProjectIndex>,
) -> (Option<CompletionResponse>, Option<String>) {
    if let Some(ctx) = import_path_at(&state.source, line, col) {
        let mut items = build_import_completions(&ctx.prefix, &state.uri);
        let is_relative = ctx.prefix.starts_with('.') || ctx.specifier.starts_with('.');
        for item in &mut items {
            let full_label = item.label.clone();
            let insert_text = imports::import_insert_text(&full_label);
            if is_relative {
                item.detail = Some(full_label.clone());
                item.label = insert_text.clone();
                item.kind = Some(CompletionItemKind::MODULE);
            }
            item.filter_text = Some(full_label);
            item.text_edit = None;
            item.insert_text = Some(insert_text);
        }
        let log = format!(
            "completion({}:{})  import-path  trigger_kind={} trigger_char={:?} items={}",
            line + 1,
            col + 1,
            trigger_kind,
            trigger_char,
            items.len(),
        );
        let resp = CompletionResponse::List(CompletionList {
            is_incomplete: true,
            items,
        });
        return (Some(resp), Some(log));
    }

    if let Some(module_path) = named_import_module_at(&state.source, line, col) {
        let already_imported = named_imported_names_at(&state.source, line, col);
        let doc_uri = state.uri.clone();
        let items: Vec<_> = build_module_export_completions(&module_path, &doc_uri)
            .into_iter()
            .filter(|item| !already_imported.contains(&item.label))
            .collect();
        let log = format!(
            "completion({}:{})  named-import module_path={:?} items={}",
            line + 1,
            col + 1,
            module_path,
            items.len()
        );
        return (Some(CompletionResponse::Array(items)), Some(log));
    }

    if cursor_in_string(&state.source, line, col) {
        let log = format!(
            "completion({}:{})  inside-string → suppressed",
            line + 1,
            col + 1
        );
        return (None, Some(log));
    }

    // 1. Reflection & Static Operator `::`
    if let Some(receiver_name) = reflection::colon_colon_receiver(state, line, col, trigger_char) {
        let items = reflection::build_reflection_completions(state, &receiver_name);
        let log = format!(
            "completion({}:{})  reflection(::) receiver={:?} → {} items",
            line + 1,
            col + 1,
            receiver_name,
            items.len()
        );
        return (Some(CompletionResponse::Array(items)), Some(log));
    }

    // 2. Member Access `.` and `?.`
    if let Some(info) = members::dot_receiver(state, line, col, trigger_char) {
        let mut items = build_member_completions(state, info, true);
        let postfix_items = build_postfix_completions(state, line, col);
        items.extend(postfix_items);
        let log = format!(
            "completion({}:{})  dot  → {} members + postfix",
            line + 1,
            col + 1,
            items.len()
        );
        return (Some(CompletionResponse::Array(items)), Some(log));
    }

    let postfix_items = build_postfix_completions(state, line, col);
    if !postfix_items.is_empty() {
        let log = format!(
            "completion({}:{})  postfix-only  → {} items",
            line + 1,
            col + 1,
            postfix_items.len()
        );
        return (Some(CompletionResponse::Array(postfix_items)), Some(log));
    }

    if let Some(info) = members::pattern_receiver(state, line, col) {
        let items = build_member_completions(state, info, false);
        let log = format!(
            "completion({}:{})  pattern  → {} members",
            line + 1,
            col + 1,
            items.len()
        );
        return (Some(CompletionResponse::Array(items)), Some(log));
    }

    if let Some(items) = calls::build_call_argument_completions(state, line, col) {
        let log = format!(
            "completion({}:{})  call_arguments  → {} items",
            line + 1,
            col + 1,
            items.len()
        );
        return (Some(CompletionResponse::Array(items)), Some(log));
    }

    let mut items = build_completions(state, line, col);

    if let Some(idx) = index {
        let prefix = get_word_prefix(&state.source, line, col);
        if prefix.is_some() {
            let already_known: std::collections::HashSet<String> =
                state.symbol_map.keys().cloned().collect();
            let auto = autoimport::build_autoimport_completions(
                &state.source,
                &state.uri,
                idx,
                &already_known,
                prefix.as_deref(),
            );
            items.extend(auto);
        }
    }

    let log = format!(
        "completion({}:{})  general  → {} items",
        line + 1,
        col + 1,
        items.len()
    );
    (Some(CompletionResponse::Array(items)), Some(log))
}

fn get_word_prefix(source: &str, line: u32, col: u32) -> Option<String> {
    let line_str = source.lines().nth(line as usize)?;
    let col_idx = (col as usize).min(line_str.len());
    let prefix = &line_str[..col_idx];
    let word = prefix.rsplit(|c: char| !c.is_alphanumeric() && c != '_').next()?.trim();
    if word.is_empty() {
        None
    } else {
        Some(word.to_string())
    }
}

pub fn build_completions(state: &DocumentState, line: u32, col: u32) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::with_capacity(160);

    let scope_items = scope::build_scope_completions(state, line, col);
    items.extend(scope_items);

    for (idx, kw) in keywords::KEYWORDS.iter().enumerate() {
        items.push(CompletionItem {
            label: kw.label.into(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: kw.detail.map(str::to_owned),
            documentation: kw.doc.map(|d| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: d.into(),
                })
            }),
            insert_text: kw.snippet.map(str::to_owned),
            insert_text_format: if kw.snippet.is_some() {
                Some(InsertTextFormat::SNIPPET)
            } else {
                None
            },
            sort_text: Some(format!("2_{:02}_{}", idx, kw.label)),
            ..Default::default()
        });
    }

    items
}

fn cursor_in_string(source: &str, line: u32, col: u32) -> bool {
    let src_line = match source.lines().nth(line as usize) {
        Some(l) => l,
        None => return false,
    };
    let bytes = src_line.as_bytes();
    let col = (col as usize).min(bytes.len());

    let mut in_string = false;
    let mut quote_char = b'"';
    let mut i = 0;
    while i < col {
        let c = bytes[i];
        if !in_string {
            if c == b'"' || c == b'\'' || c == b'`' {
                in_string = true;
                quote_char = c;
            }
        } else if c == b'\\' {
            i += 2;
            continue;
        } else if c == quote_char {
            in_string = false;
        }
        i += 1;
    }
    in_string
}
