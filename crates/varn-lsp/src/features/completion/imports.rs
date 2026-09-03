use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};
use varn_checker::module_resolver::ImportResolver;

use crate::constants::STD_PREFIX;
use crate::document::import::uri_to_path;
use crate::util::kinds::to_completion_kind;

pub fn build_import_completions(prefix: &str, doc_uri: &str) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();

    if prefix.is_empty() || prefix.starts_with(STD_PREFIX) {
        items.extend(stdlib_module_completions(prefix));
    }

    if !prefix.starts_with(STD_PREFIX) {
        items.extend(relative_varn_completions(prefix, doc_uri));
    }

    items
}

fn stdlib_module_completions(prefix: &str) -> Vec<CompletionItem> {
    let Some(provider) = varn_modules::provider::get() else {
        return Vec::new();
    };
    provider
        .all_specs()
        .iter()
        .filter(|m| matches!(m.kind, varn_modules::ModuleKind::Stdlib) && m.id.starts_with(prefix))
        .map(|m| CompletionItem {
            label: m.id.to_owned(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("stdlib module".into()),
            ..Default::default()
        })
        .collect()
}

fn relative_varn_completions(prefix: &str, doc_uri: &str) -> Vec<CompletionItem> {
    use std::path::Path;

    let doc_path = uri_to_path(doc_uri);
    let doc_file = Path::new(&doc_path);
    let base_dir = match doc_file.parent() {
        Some(d) => d.to_path_buf(),
        None => return Vec::new(),
    };

    let (scan_dir, path_so_far) = if prefix.is_empty() || prefix == "." || prefix == "./" {
        (base_dir.clone(), "./".to_owned())
    } else if prefix.ends_with('/') {
        (base_dir.join(prefix), prefix.to_owned())
    } else {
        let p = Path::new(prefix);
        match p.parent().filter(|par| !par.as_os_str().is_empty()) {
            Some(parent) => {
                let dir_prefix = format!("{}/", parent.to_string_lossy().replace('\\', "/"));
                (base_dir.join(parent), dir_prefix)
            }
            None => (base_dir.clone(), "./".to_owned()),
        }
    };

    let mut items = Vec::new();
    let read_dir = match std::fs::read_dir(&scan_dir) {
        Ok(rd) => rd,
        Err(_) => return items,
    };

    for entry in read_dir.flatten() {
        let entry_path = entry.path();
        let file_name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        if entry_path.is_dir() {
            let label = format!("{path_so_far}{file_name}/");
            if label.starts_with(prefix) {
                items.push(CompletionItem {
                    label,
                    kind: Some(CompletionItemKind::FOLDER),
                    ..Default::default()
                });
            }
        } else if file_name.ends_with(".vn") {
            let stem = &file_name[..file_name.len() - ".vn".len()];
            if doc_file.file_stem().and_then(|s| s.to_str()) == Some(stem) {
                continue;
            }
            let label = format!("{path_so_far}{stem}");
            if label.starts_with(prefix) {
                items.push(CompletionItem {
                    label,
                    kind: Some(CompletionItemKind::FILE),
                    detail: Some(file_name.clone()),
                    ..Default::default()
                });
            }
        }
    }

    items
}

pub fn build_module_export_completions(module_path: &str, doc_uri: &str) -> Vec<CompletionItem> {
    if module_path.is_empty() {
        return Vec::new();
    }
    if module_path.starts_with('.') || module_path.starts_with('/') {
        build_relative_export_completions(module_path, doc_uri)
    } else {
        build_stdlib_export_completions(module_path)
    }
}

fn build_stdlib_export_completions(module_path: &str) -> Vec<CompletionItem> {
    let exports = crate::workspace::resolver::with_resolver(|r| r.stdlib_exports(module_path))
        .as_ref()
        .clone();
    let mut items: Vec<CompletionItem> = exports
        .into_iter()
        .filter(|(name, _)| !name.contains('.'))
        .map(|(name, sym)| {
            let kind = Some(to_completion_kind(sym.kind));
            CompletionItem {
                label: name,
                kind,
                ..Default::default()
            }
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn build_relative_export_completions(module_path: &str, doc_uri: &str) -> Vec<CompletionItem> {
    use std::path::Path;

    let doc_path = uri_to_path(doc_uri);
    let doc_dir = match Path::new(&doc_path).parent() {
        Some(d) => d.to_path_buf(),
        None => return Vec::new(),
    };

    let joined = doc_dir.join(module_path);
    let with_ext = if joined.extension().is_none() {
        joined.with_extension("vn")
    } else {
        joined.clone()
    };
    let abs_str = if with_ext.exists() {
        with_ext.to_string_lossy().into_owned()
    } else {
        joined.to_string_lossy().into_owned()
    };

    let exports =
        crate::workspace::resolver::with_resolver(|r| r.module_exports(&abs_str, &mut Vec::new()))
            .as_ref()
            .clone();

    let mut items: Vec<CompletionItem> = exports
        .into_iter()
        .filter(|(name, _)| !name.contains('.'))
        .map(|(name, sym)| {
            let kind = Some(to_completion_kind(sym.kind));
            CompletionItem {
                label: name,
                kind,
                ..Default::default()
            }
        })
        .collect();

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

pub fn resolve_relative_module_debug(module_path: &str, doc_uri: &str) -> Option<String> {
    use std::path::Path;

    let doc_path = uri_to_path(doc_uri);
    let doc_dir = Path::new(&doc_path).parent()?.to_path_buf();

    let joined = doc_dir.join(module_path);
    let with_ext = if joined.extension().is_none() {
        joined.with_extension("vn")
    } else {
        joined.clone()
    };

    if with_ext.exists() {
        Some(with_ext.to_string_lossy().into_owned())
    } else {
        Some(joined.to_string_lossy().into_owned())
    }
}

pub fn import_insert_text(label: &str) -> String {
    let trailing = if label.ends_with('/') { "/" } else { "" };
    let label_no_slash = label.trim_end_matches('/');
    label_no_slash
        .rfind('/')
        .map(|idx| format!("{}{}", &label_no_slash[idx + 1..], trailing))
        .unwrap_or_else(|| label.to_owned())
}
