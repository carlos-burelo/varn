mod autoimport;
mod calls;
mod columns;
mod imports;
mod keywords;
mod lexical;
pub mod members;
pub mod postfix;
pub mod probe;
pub mod receiver_literals;
pub mod receivers;
use rustc_hash::FxHashSet;
pub mod reflection;
pub(crate) mod scope;
mod site;

use std::time::Instant;
use tower_lsp_f::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionResponse, Documentation,
    InsertTextFormat, MarkupContent, MarkupKind, Position, Range, TextEdit,
};

use crate::document::{
    import_path_at, named_import_module_at, named_imported_names_at, DocumentState,
};
use crate::index::ProjectIndex;
use vn_sem::semantic_info::ResolvedMemberKind;

pub use imports::{
    build_import_completions, build_module_export_completions, resolve_relative_module_debug,
};
pub use members::build_member_completions;
pub use postfix::build_postfix_completions;
pub use receivers::{dot_receiver, pattern_receiver, DotVerdict};
pub use reflection::build_reflection_completions;

pub fn patched_source_for_member(
    state: &DocumentState,
    line: u32,
    col: u32,
    trigger_char: Option<&str>,
) -> Option<probe::ProbedSource> {
    let cursor = state.offset_at_line_col(line, col) as usize;
    let before = state.source.get(..cursor)?;
    let after = state.source.get(cursor..)?;
    let continues_word = after
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_');
    let ends_with_access = before.ends_with('.') || before.ends_with("?.");
    if !ends_with_access || continues_word {
        return None;
    }
    let verdict = receivers::dot_receiver(state, line, col, trigger_char);
    if let Some(info) = &verdict.info
        && (!info.is_instance || !matches!(verdict.stage, "spatial" | "scope"))
    {
        return None;
    }
    if let Some(probed) = probe::probed_source(state, cursor as u32) {
        return Some(probed);
    }
    if verdict.info.is_some() {
        return None;
    }
    Some(probe::ProbedSource {
        text: format!("{before}{}{after}", probe::MEMBER_PLACEHOLDER),
        line,
        character: col,
    })
}

pub fn build_completion_response(
    state: &DocumentState,
    resolver: &vn_resolver::ModuleResolver,
    line: u32,
    col: u32,
    trigger_char: Option<&str>,
    trigger_kind: String,
    index: Option<&ProjectIndex>,
) -> (Option<CompletionResponse>, Option<String>) {
    let start = Instant::now();
    let ms = || start.elapsed().as_millis();
    let col_utf16 = col;
    let col = columns::byte_column(&state.source, line, col_utf16);
    let prefix = get_word_prefix(&state.source, line, col).unwrap_or_default();
    let trig = compact_trigger(trigger_char, &trigger_kind);
    let lc = format!("{}:{}", line + 1, col + 1);
    if let Some(ctx) = import_path_at(&state.source, &state.tokens, line, col) {
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
                                character: columns::utf16_column(
                                    &state.source,
                                    line,
                                    ctx.content_start_col,
                                ),
                            },
                            end: Position {
                                line,
                                character: col_utf16,
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

    if let Some(module_path) = named_import_module_at(&state.source, &state.tokens, line, col) {
        let already_imported = named_imported_names_at(&state.source, &state.tokens, line, col);
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

    let cursor_offset = state.offset_at_line_col(line, col_utf16) as usize;
    let context = lexical::lexical_context(&state.source, cursor_offset);
    if context != lexical::LexicalContext::Code {
        return (
            None,
            Some(format!("cmp {lc} trg={trig} suppressed={context:?}")),
        );
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

    let verdict = receivers::dot_receiver(state, line, col_utf16, trigger_char);
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
        let mut items = build_member_completions(state, info, true, &prefix, cursor_offset as u32);
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

    if let Some(info) = receivers::pattern_receiver(state, line, col) {
        let ty_text = state.db.ty_text(&info.ty);
        let items = build_member_completions(state, info, false, &prefix, cursor_offset as u32);
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

    let named_arguments = calls::build_call_argument_completions(state, line, col);

    let site = site::completion_site(state, cursor_offset as u32);
    if let site::CompletionSite::ObjectKeys { type_name, present } = &site {
        let items = object_key_completions(state, type_name, present);
        let log = format!(
            "cmp {lc} trg={trig} pre={prefix:?} object-keys type={type_name} items={} {}ms",
            items.len(),
            ms()
        );
        return (
            Some(CompletionResponse::CompletionItemList(items)),
            Some(log),
        );
    }

    let (mut items, scope_n) = build_completions(state, line, col, cursor_offset as u32);
    let (items_kept, scope_n) = restrict_to_site(items, scope_n, &site);
    items = items_kept;
    let named_n = named_arguments.as_ref().map_or(0, Vec::len);
    if let Some(named) = named_arguments {
        items.splice(0..0, named);
    }
    let kw_n = items.len() - scope_n - named_n;

    let mut auto_n = 0;
    if let Some(idx) = index
        && !prefix.is_empty()
    {
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
            if let Some(f) = &i.filter_text
                && f != &i.label
            {
                s.push_str(&format!("?{f}"));
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

fn keyword_contexts(label: &str) -> u8 {
    keywords::KEYWORDS
        .iter()
        .find(|entry| entry.label == label)
        .map_or(0, |entry| entry.contexts)
}

fn restrict_to_site(
    items: Vec<CompletionItem>,
    scope_n: usize,
    site: &site::CompletionSite,
) -> (Vec<CompletionItem>, usize) {
    use site::CompletionSite as Site;
    let (scope_kinds, keyword_mask): (Option<&[CompletionItemKind]>, u8) = match site {
        Site::Statement => (None, keywords::STATEMENT | keywords::EXPRESSION),
        Site::Expression | Site::ObjectKeys { .. } => (None, keywords::EXPRESSION),
        Site::ClassBody => (Some(&[]), keywords::MEMBER),
        Site::InterfaceBody => (Some(&[]), keywords::INTERFACE_MEMBER),
        Site::EnumBody => (Some(&[]), 0),
        Site::AfterExport => (Some(&[]), keywords::EXPORT),
        Site::Type => (
            Some(&[
                CompletionItemKind::Class,
                CompletionItemKind::Interface,
                CompletionItemKind::Enum,
                CompletionItemKind::TypeParameter,
                CompletionItemKind::Module,
            ]),
            keywords::TYPE,
        ),
        Site::NewTarget | Site::ExtendsClass => (Some(&[CompletionItemKind::Class]), 0),
        Site::ExtendsInterface | Site::Implements => (Some(&[CompletionItemKind::Interface]), 0),
    };
    let mut kept = Vec::with_capacity(items.len());
    let mut kept_scope = 0;
    for (index, item) in items.into_iter().enumerate() {
        let is_scope = index < scope_n;
        let keep = if is_scope {
            scope_kinds.is_none_or(|kinds| item.kind.is_some_and(|kind| kinds.contains(&kind)))
        } else {
            keyword_contexts(&item.label) & keyword_mask != 0
        };
        if keep {
            if is_scope {
                kept_scope += 1;
            }
            kept.push(item);
        }
    }
    (kept, kept_scope)
}

fn object_key_completions(
    state: &DocumentState,
    type_name: &str,
    present: &[String],
) -> Vec<CompletionItem> {
    let ty = state.db.named_type(type_name);
    state
        .members_of_type(&ty)
        .into_iter()
        .filter(|m| !m.is_static)
        .filter(|m| m.kind != ResolvedMemberKind::Constructor)
        .filter(|m| !present.iter().any(|key| key.as_str() == m.name.as_ref()))
        .map(|m| CompletionItem {
            label: m.name.to_string(),
            kind: Some(CompletionItemKind::Field),
            detail: Some(state.ty_text(&m.ty)),
            insert_text: Some(format!("{}: $0", m.name)),
            insert_text_format: Some(InsertTextFormat::Snippet),
            sort_text: Some(format!("0_{}", m.name)),
            ..Default::default()
        })
        .collect()
}

pub fn build_completions(
    state: &DocumentState,
    line: u32,
    col: u32,
    cursor_offset: u32,
) -> (Vec<CompletionItem>, usize) {
    let mut items: Vec<CompletionItem> = Vec::with_capacity(160);

    let prefix = get_word_prefix(&state.source, line, col).unwrap_or_default();
    let scope_items = scope::build_scope_completions(state, cursor_offset, &prefix);
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
