use std::collections::HashSet;

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position, Range, TextEdit};
use varn_checker::SymbolKind;

use crate::constants::{
    SORT_AUTOIMPORT, STDLIB_STD_PATH, STD_LIB_PATH_SEGMENT, STD_PREFIX, VARN_EXTENSION,
};
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
    uri.contains(STD_LIB_PATH_SEGMENT)
}

fn uri_to_specifier(from_uri: &str, target_uri: &str) -> String {
    let target_path = uri_to_path(target_uri);
    let normalized = target_path.replace('\\', "/");

    if let Some(idx) = normalized.find(STDLIB_STD_PATH) {
        let rest = &normalized[idx + STDLIB_STD_PATH.len()..];
        let mod_suffix = concat!("/mod", ".vn"); // avoids literal ".vn" in non-constant position
        let module = rest
            .strip_suffix(mod_suffix)
            .or_else(|| rest.strip_suffix(VARN_EXTENSION))
            .unwrap_or(rest);
        return format!("{STD_PREFIX}{module}");
    }

    let from_path = uri_to_path(from_uri);
    relative_import_path(&from_path, &target_path)
}

fn relative_import_path(from_file: &str, to_file: &str) -> String {
    let from = from_file.replace('\\', "/");
    let to = to_file.replace('\\', "/");

    let from_dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let from_parts: Vec<&str> = from_dir.split('/').filter(|p| !p.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|p| !p.is_empty()).collect();

    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = from_parts.len() - common;
    let downs = &to_parts[common..];

    let mut result = String::new();
    if ups == 0 {
        result.push_str("./");
    } else {
        for _ in 0..ups {
            result.push_str("../");
        }
    }
    result.push_str(&downs.join("/"));

    if result.ends_with(VARN_EXTENSION) {
        result.truncate(result.len() - VARN_EXTENSION.len());
    }

    result
}
