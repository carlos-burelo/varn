use std::collections::HashSet;

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position, Range, TextEdit};
use varn_checker::SymbolKind;

use varn_modules::resolver::{normalize_display_path, relative_import_path};

use crate::constants::{SORT_AUTOIMPORT, STDLIB_STD_PATH, STD_PREFIX};
use crate::document::import::uri_to_path;
use crate::index::ProjectIndex;

pub fn build_autoimport_completions(
    source: &str,
    doc_uri: &str,
    index: &ProjectIndex,
    already_known: &HashSet<String>,
) -> Vec<CompletionItem> {
    let insert_pos = import_insert_position(source);
    let mut items: Vec<CompletionItem> = Vec::new();

    for (name, entries) in &index.name_index {
        if already_known.contains(name) {
            continue;
        }

        let entry_opt = entries
            .iter()
            .find(|(uri, _)| uri != doc_uri && is_stdlib_uri(uri))
            .or_else(|| entries.iter().find(|(uri, _)| uri != doc_uri));

        let (target_uri, entry) = match entry_opt {
            Some(e) => e,
            None => continue,
        };

        let specifier = uri_to_specifier(doc_uri, target_uri);
        let import_text = format!("import {{ {name} }} from \"{specifier}\";\n");

        let kind = Some(match entry.kind {
            SymbolKind::Function => CompletionItemKind::FUNCTION,
            SymbolKind::Class => CompletionItemKind::CLASS,
            SymbolKind::Interface => CompletionItemKind::INTERFACE,
            SymbolKind::Enum => CompletionItemKind::ENUM,
            SymbolKind::Const => CompletionItemKind::CONSTANT,
            SymbolKind::Namespace => CompletionItemKind::MODULE,
            SymbolKind::Struct => CompletionItemKind::CLASS,
            SymbolKind::TypeAlias => CompletionItemKind::CLASS,
            _ => CompletionItemKind::VARIABLE,
        });

        let type_hint = if entry.type_str.is_empty() {
            String::new()
        } else {
            format!(": {}", entry.type_str)
        };
        let detail = format!("{name}{type_hint}  ↳ \"{specifier}\"");

        items.push(CompletionItem {
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
            sort_text: Some(format!("{SORT_AUTOIMPORT}{name}")),
            filter_text: Some(name.clone()),
            ..Default::default()
        });
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
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
    varn_modules::resolver::is_varn_uri(uri)
}

fn uri_to_specifier(from_uri: &str, target_uri: &str) -> String {
    if let Some(spec) = varn_modules::resolver::specifier_from_uri(target_uri) {
        return spec;
    }

    let target_path = uri_to_path(target_uri);
    let normalized = normalize_display_path(&target_path);

    if let Some(idx) = normalized.find(STDLIB_STD_PATH) {
        let rest = &normalized[idx + STDLIB_STD_PATH.len()..];
        let mod_suffix = concat!("/mod", ".vn");
        let module = rest
            .strip_suffix(mod_suffix)
            .or_else(|| rest.strip_suffix(".vn"))
            .unwrap_or(rest);
        return format!("{STD_PREFIX}{module}");
    }

    let from_path = uri_to_path(from_uri);
    relative_import_path(&from_path, &target_path)
}
