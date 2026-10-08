mod autoimport;
mod calls;
mod imports;
mod keywords;
pub mod members;
pub mod postfix;
use rustc_hash::FxHashSet;
pub mod reflection;
pub(crate) mod scope;

use std::time::Instant;
use tower_lsp_f::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionResponse, Documentation,
    InsertTextFormat, MarkupContent, MarkupKind, Position, Range, TextEdit,
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
    resolver: &varn_checker::module_resolver::DiskResolver,
    line: u32,
    col: u32,
    trigger_char: Option<&str>,
    trigger_kind: String,
    index: Option<&ProjectIndex>,
) -> (Option<CompletionResponse>, Option<String>) {
    let start = Instant::now();
    let ms = || start.elapsed().as_millis();
    let prefix = get_word_prefix(&state.source, line, col).unwrap_or_default();
    let trig = compact_trigger(trigger_char, &trigger_kind);
    let lc = format!("{}:{}", line + 1, col + 1);
    if let Some(ctx) = import_path_at(&state.source, line, col) {
        let mut items = build_import_completions(&ctx.prefix, &state.uri);
        let is_relative = ctx.prefix.starts_with('.') || ctx.specifier.starts_with('.');
        for item in &mut items {
            let full_label = item.label.clone();
            if is_relative {
                let insert_text = imports::import_insert_text(&full_label);
                item.detail = Some(full_label.clone());
                item.label = insert_text.clone();
                item.kind = Some(CompletionItemKind::Module);
                item.filter_text = Some(full_label);
                item.text_edit = None;
                item.insert_text = Some(insert_text);
            } else {
                item.filter_text = Some(full_label.clone());
                item.text_edit = Some(tower_lsp_f::lsp_types::CompletionItemTextEdit::TextEdit(
                    TextEdit {
                        range: Range {
                            start: Position {
                                line,
                                character: ctx.content_start_col,
                            },
                            end: Position {
                                line,
                                character: col,
                            },
                        },
                        new_text: full_label,
                    },
                ));
                item.insert_text = None;
            }
        }
        let log = format!(
            "cmp {lc} trg={trig} pre={prefix:?} ipath spec={:?} items={} {}ms",
            ctx.specifier,
            items.len(),
            ms(),
        );
        let resp = CompletionResponse::CompletionList(CompletionList {
            is_incomplete: true,
            item_defaults: None,
            apply_kind: None,
            items,
        });
        return (Some(resp), Some(log));
    }

    if let Some(module_path) = named_import_module_at(&state.source, line, col) {
        let already_imported = named_imported_names_at(&state.source, line, col);
        let doc_uri = state.uri.clone();
        let all = build_module_export_completions(resolver, &module_path, &doc_uri);
        let total = all.len();
        let items: Vec<_> = all
            .into_iter()
            .filter(|item| !already_imported.contains(&item.label))
            .collect();
        let log = format!(
            "cmp {lc} trg={trig} pre={prefix:?} nimport mod={module_path:?} exp={total} kept={} show=[{}] {}ms",
            items.len(),
            preview_items(&items, 5),
            ms()
        );
        return (
            Some(CompletionResponse::CompletionItemList(items)),
            Some(log),
        );
    }

    if cursor_in_string(&state.source, line, col) {
        return (None, Some(format!("cmp {lc} trg={trig} suppressed=str")));
    }

    if let Some((receiver_name, receiver_offset)) =
        reflection::colon_colon_receiver(state, line, col, trigger_char)
    {
        let items = reflection::build_reflection_completions(
            state,
            &receiver_name,
            receiver_offset,
            &prefix,
        );
        let log = format!(
            "cmp {lc} trg={trig} pre={prefix:?} ccref={receiver_name:?} items={} {}ms",
            items.len(),
            ms()
        );
        return (
            Some(CompletionResponse::CompletionItemList(items)),
            Some(log),
        );
    }

    let verdict = members::dot_receiver(state, line, col, trigger_char);
    let dot_miss = verdict.info.is_none().then(|| {
        format!(
            "dotmiss={}({})",
            verdict.stage,
            verdict.receiver.chars().take(24).collect::<String>()
        )
    });
    if let Some(info) = verdict.info {
        let ty_text = state.db.ty_text(&info.ty);
        let inst = info.is_instance;
        let mut items = build_member_completions(state, info, true, &prefix);
        let mem = items.len();
        let postfix_items = build_postfix_completions(state, line, col);
        let post = postfix_items.len();
        items.extend(postfix_items);
        let log = format!(
            "cmp {lc} trg={trig} pre={prefix:?} dot via={} recv={:?} ty={ty_text} inst={inst} mem={mem} post={post} tot={} show=[{}] {}ms",
            verdict.stage,
            verdict.receiver,
            items.len(),
            preview_items(&items, 8),
            ms()
        );
        return (
            Some(CompletionResponse::CompletionItemList(items)),
            Some(log),
        );
    }

    let postfix_items = build_postfix_completions(state, line, col);
    if !postfix_items.is_empty() {
        let log = format!(
            "cmp {lc} trg={trig} pre={prefix:?} postfix-only items={} {}ms",
            postfix_items.len(),
            ms()
        );
        return (
            Some(CompletionResponse::CompletionItemList(postfix_items)),
            Some(log),
        );
    }

    if let Some(info) = members::pattern_receiver(state, line, col) {
        let ty_text = state.db.ty_text(&info.ty);
        let items = build_member_completions(state, info, false, &prefix);
        let log = format!(
            "cmp {lc} trg={trig} pre={prefix:?} pattern ty={ty_text} items={} {}ms",
            items.len(),
            ms()
        );
        return (
            Some(CompletionResponse::CompletionItemList(items)),
            Some(log),
        );
    }

    if let Some(items) = calls::build_call_argument_completions(state, line, col) {
        let log = format!(
            "cmp {lc} trg={trig} pre={prefix:?} callargs items={} {}ms",
            items.len(),
            ms()
        );
        return (
            Some(CompletionResponse::CompletionItemList(items)),
            Some(log),
        );
    }

    let (mut items, scope_n) = build_completions(state, line, col);
    let kw_n = items.len() - scope_n;

    let mut auto_n = 0;
    if let Some(idx) = index {
        if !prefix.is_empty() {
            let already_known: FxHashSet<String> = state.symbol_map.keys().cloned().collect();
            let auto = autoimport::build_autoimport_completions(
                &state.source,
                &state.uri,
                idx,
                &already_known,
                Some(&prefix),
            );
            auto_n = auto.len();
            items.extend(auto);
        }
    }

    let line_txt: String = state
        .source
        .lines()
        .nth(line as usize)
        .unwrap_or("")
        .trim()
        .chars()
        .take(64)
        .collect();
    let log = format!(
        "cmp {lc} trg={trig} pre={prefix:?} line={line_txt:?} general scope={scope_n} kw={kw_n} auto={auto_n} tot={} show=[{}] {} {}ms",
        items.len(),
        preview_items(&items, 8),
        dot_miss.unwrap_or_default(),
        ms()
    );
    (
        Some(CompletionResponse::CompletionItemList(items)),
        Some(log),
    )
}

pub fn preview_items(items: &[tower_lsp_f::lsp_types::CompletionItem], n: usize) -> String {
    let mut sorted: Vec<_> = items.iter().collect();
    sorted.sort_by(|a, b| {
        a.sort_text
            .as_deref()
            .unwrap_or("~")
            .cmp(b.sort_text.as_deref().unwrap_or("~"))
            .then_with(|| a.label.cmp(&b.label))
    });
    sorted
        .into_iter()
        .take(n)
        .map(|i| {
            let mut s = format!(
                "{}({})",
                i.label.as_str(),
                i.sort_text.as_deref().unwrap_or("~")
            );
            if let Some(te) = &i.text_edit {
                match te {
                    tower_lsp_f::lsp_types::CompletionItemTextEdit::TextEdit(e) => {
                        s.push_str(&format!(
                            "~>{}:{}-{}:{}={}",
                            e.range.start.line,
                            e.range.start.character,
                            e.range.end.line,
                            e.range.end.character,
                            e.new_text
                        ));
                    }
                    tower_lsp_f::lsp_types::CompletionItemTextEdit::InsertReplaceEdit(e) => {
                        s.push_str(&format!(
                            "~>{}:{}-{}:{}={}",
                            e.insert.start.line,
                            e.insert.start.character,
                            e.replace.end.line,
                            e.replace.end.character,
                            e.new_text
                        ));
                    }
                }
            }
            if let Some(f) = &i.filter_text {
                if f != &i.label {
                    s.push_str(&format!("?{f}"));
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn compact_trigger(trigger_char: Option<&str>, trigger_kind: &str) -> String {
    if let Some(c) = trigger_char {
        return format!("'{c}'");
    }
    if trigger_kind.contains("Invoked") {
        "inv".to_owned()
    } else {
        "re".to_owned()
    }
}

fn get_word_prefix(source: &str, line: u32, col: u32) -> Option<String> {
    let line_str = source.lines().nth(line as usize)?;
    let col_idx = (col as usize).min(line_str.len());
    let prefix = &line_str[..col_idx];
    let word = prefix
        .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?
        .trim();
    if word.is_empty() {
        None
    } else {
        Some(word.to_string())
    }
}

pub fn build_completions(
    state: &DocumentState,
    line: u32,
    col: u32,
) -> (Vec<CompletionItem>, usize) {
    let mut items: Vec<CompletionItem> = Vec::with_capacity(160);

    let prefix = get_word_prefix(&state.source, line, col).unwrap_or_default();
    let scope_items = scope::build_scope_completions(state, line, col, &prefix);
    let scope_n = scope_items.len();
    items.extend(scope_items);

    for (idx, kw) in keywords::KEYWORDS.iter().enumerate() {
        items.push(CompletionItem {
            label: kw.label.into(),
            kind: Some(CompletionItemKind::Keyword),
            detail: kw.detail.map(str::to_owned),
            documentation: kw.doc.map(|d| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: d.into(),
                })
            }),
            insert_text: kw.snippet.map(str::to_owned),
            insert_text_format: if kw.snippet.is_some() {
                Some(InsertTextFormat::Snippet)
            } else {
                None
            },
            sort_text: Some(format!("3_{:02}_{}", idx, kw.label)),
            ..Default::default()
        });
    }

    (items, scope_n)
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
