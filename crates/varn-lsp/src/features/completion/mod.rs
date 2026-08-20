mod autoimport;
mod calls;
mod imports;
mod keywords;
pub mod members;
mod scope;

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionResponse, Documentation,
    InsertTextFormat, MarkupContent, MarkupKind,
};
use varn_checker::SymbolKind;

use crate::constants::STDLIB_LINE_MARKER;
use crate::document::{
    import_path_at, named_import_module_at, named_imported_names_at, DocumentState,
};
use crate::index::ProjectIndex;
use crate::util::converters::to_completion_kind;
use crate::util::ranking::symbol_priority;

pub use imports::{
    build_import_completions, build_module_export_completions, resolve_relative_module_debug,
};
pub use members::build_member_completions;

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

    if let Some(info) = members::dot_receiver(state, line, col, trigger_char) {
        let items = build_member_completions(state, info, true);
        let log = format!(
            "completion({}:{})  dot  → {} members",
            line + 1,
            col + 1,
            items.len()
        );
        return (Some(CompletionResponse::Array(items)), Some(log));
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

    let mut items = build_completions(state, line);

    if let Some(idx) = index {
        let already_known: std::collections::HashSet<String> =
            state.symbol_map.keys().cloned().collect();
        let auto = autoimport::build_autoimport_completions(
            &state.source,
            &state.uri,
            idx,
            &already_known,
        );
        items.extend(auto);
    }

    let log = format!(
        "completion({}:{})  general  → {} items",
        line + 1,
        col + 1,
        items.len()
    );
    (Some(CompletionResponse::Array(items)), Some(log))
}

pub fn build_completions(state: &DocumentState, line: u32) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::with_capacity(160);

    let scope_items = scope::build_scope_completions(state, line);
    let scope_names: std::collections::HashSet<String> =
        scope_items.iter().map(|i| i.label.clone()).collect();
    items.extend(scope_items);

    for kw in keywords::KEYWORDS {
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
            ..Default::default()
        });
    }

    let mut seen: std::collections::HashMap<&str, &crate::document::SymbolRecord> =
        std::collections::HashMap::new();
    for sym in &state.symbols {
        if sym.kind == SymbolKind::Parameter {
            continue;
        }

        if scope_names.contains(&sym.name) {
            continue;
        }
        seen.entry(&sym.name)
            .and_modify(|prev| {
                if symbol_priority(sym.kind) < symbol_priority(prev.kind) {
                    *prev = sym;
                }
            })
            .or_insert(sym);
    }

    for sym in seen.values() {
        let detail = if sym.type_str.is_empty() {
            None
        } else {
            Some(sym.type_str.clone())
        };
        let (insert_text, insert_text_format) =
            if sym.kind == SymbolKind::Function && sym.line == STDLIB_LINE_MARKER {
                (
                    Some(format!("{}($0)", sym.name)),
                    Some(InsertTextFormat::SNIPPET),
                )
            } else {
                (None, None)
            };
        items.push(CompletionItem {
            label: sym.name.clone(),
            kind: Some(to_completion_kind(sym.kind)),
            detail,
            insert_text,
            insert_text_format,
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
