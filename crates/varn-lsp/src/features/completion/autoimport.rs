use rustc_hash::FxHashSet;

use tower_lsp_f::lsp_types::{CompletionItem, Position, Range, TextEdit};

use varn_modules::resolver::relative_import_path;

use crate::constants::SORT_AUTOIMPORT;
use crate::document::import::uri_to_path;
use crate::index::ProjectIndex;
use crate::util::kinds::to_completion_kind;

pub fn build_autoimport_completions(
    source: &str,
    doc_uri: &str,
    index: &ProjectIndex,
    already_known: &FxHashSet<String>,
    prefix_filter: Option<&str>,
) -> Vec<CompletionItem> {
    let insert_pos = import_insert_position(source);

    let filter_lower = prefix_filter
        .map(|p| p.to_lowercase())
        .filter(|p| !p.is_empty());

    let Some(query) = filter_lower else {
        return Vec::new();
    };

    const MAX_AUTOIMPORT_ITEMS: usize = 50;

    let mut ranked: Vec<(u8, String, CompletionItem)> = Vec::new();

    for (name, entries) in &index.name_index {
        if already_known.contains(name) {
            continue;
        }

        let name_lower = entries
            .first()
            .map(|e| e.name_lower.as_str())
            .unwrap_or(name.as_str());
        let rank = if name_lower.starts_with(&query) {
            0u8
        } else if name_lower.contains(&query) {
            1u8
        } else {
            continue;
        };

        let entry_opt = entries
            .iter()
            .find(|e| e.uri.as_ref() != doc_uri && is_stdlib_uri(&e.uri))
            .or_else(|| entries.iter().find(|e| e.uri.as_ref() != doc_uri));

        let Some(entry) = entry_opt else {
            continue;
        };

        let specifier = uri_to_specifier(doc_uri, &entry.uri);
        let Some(specifier) = specifier else {
            continue;
        };
        let import_text = format!("import {{ {name} }} from \"{specifier}\";\n");

        let kind = Some(to_completion_kind(entry.kind));

        let type_hint = if entry.type_str.is_empty() {
            String::new()
        } else {
            format!(": {}", entry.type_str)
        };
        let detail = format!("{name}{type_hint}  ↳ \"{specifier}\"");

        ranked.push((
            rank,
            name.clone(),
            CompletionItem {
                label: name.clone(),
                kind,
                detail: Some(detail),
                additional_text_edits: Some(vec![TextEdit {
                    range: Range {
                        start: insert_pos,
                        end: insert_pos,
                    },
                    new_text: import_text,
                }]),
                sort_text: Some(format!("{SORT_AUTOIMPORT}{rank}_{name}")),
                filter_text: Some(name.clone()),
                ..Default::default()
            },
        ));
    }

    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    ranked.truncate(MAX_AUTOIMPORT_ITEMS);
    ranked.into_iter().map(|(_, _, item)| item).collect()
}

fn import_insert_position(source: &str) -> Position {
    let mut last_import_line: i64 = -1;
    for (i, line) in source.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("import") || t.starts_with("export") || t.contains(" from ") {
            last_import_line = i as i64;
        } else if last_import_line >= 0 && !t.is_empty() {
            break;
        }
    }
    Position {
        line: (last_import_line + 1) as u32,
        character: 0,
    }
}

fn is_stdlib_uri(uri: &str) -> bool {
    crate::workspace::std_sources::is_mirrored_uri(uri)
}

fn uri_to_specifier(from_uri: &str, target_uri: &str) -> Option<String> {
    let target_path = uri_to_path(target_uri);
    if let Some(spec) = crate::workspace::std_sources::specifier_from_path(&target_path) {
        return Some(spec);
    }

    let from_path = uri_to_path(from_uri);
    let rel = relative_import_path(&from_path, &target_path);
    if rel.is_empty() || rel.contains(":/") || rel.starts_with('/') || rel.starts_with('\\') {
        return None;
    }
    Some(rel)
}
