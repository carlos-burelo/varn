use varn_checker::SymbolKind;
use varn_modules::resolver::path_to_uri;
use varn_modules::spec::{CORE_PREFIX, STD_PREFIX};

use crate::document::{import::uri_to_path, DocumentState, MemberRecord};
use crate::util::kinds::member_to_symbol_kind;

use super::{ExportEntry, ProjectIndex};

pub fn index_file(index: &mut ProjectIndex, uri: &str, state: &DocumentState) {
    let mut exports: Vec<ExportEntry> = state
        .symbols
        .iter()
        .filter(|s| is_indexable(s.kind, s.line))
        .map(|s| ExportEntry {
            name: s.name.clone(),
            global_key: s.global_key.clone(),
            kind: s.kind,
            uri: uri.to_owned(),
            line: s.line,
            col: s.col,
            type_str: s.type_str.clone(),
            doc: s.doc.clone(),
        })
        .collect();

    for sym in state
        .symbols
        .iter()
        .filter(|s| is_indexable(s.kind, s.line))
    {
        collect_member_exports(uri, &sym.name, &sym.members, &mut exports);
    }

    for export in &exports {
        index
            .name_index
            .entry(export.name.clone())
            .or_default()
            .push((uri.to_owned(), export.clone()));
        index
            .key_index
            .entry(export.global_key.clone())
            .or_default()
            .push((uri.to_owned(), export.clone()));
    }

    index.module_exports.insert(uri.to_owned(), exports);

    let doc_path = uri_to_path(uri);
    let doc_dir = std::path::Path::new(&doc_path)
        .parent()
        .map(|p| p.to_path_buf());

    for specifier in &state.import_paths {
        let resolved_uri = resolve_specifier_to_uri(specifier, doc_dir.as_deref());
        if let Some(target_uri) = resolved_uri {
            index
                .reverse_deps
                .entry(target_uri.clone())
                .or_default()
                .insert(uri.to_owned());
            index
                .module_cache
                .entry(specifier.clone())
                .or_insert(target_uri);
        }
    }
}

fn collect_member_exports(
    uri: &str,
    parent_name: &str,
    members: &[MemberRecord],
    out: &mut Vec<ExportEntry>,
) {
    for m in members {
        if m.line != u32::MAX {
            let sid = m
                .symbol_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "none".to_owned());
            let key = format!("member:{parent_name}:{}:{sid}", m.name);
            out.push(ExportEntry {
                name: m.name.clone(),
                global_key: key,
                kind: member_to_symbol_kind(m.kind),
                uri: uri.to_owned(),
                line: m.line,
                col: m.col,
                type_str: m.type_str.clone(),
                doc: None,
            });
        }
        if !m.members.is_empty() {
            collect_member_exports(uri, &m.name, &m.members, out);
        }
    }
}

fn resolve_specifier_to_uri(specifier: &str, doc_dir: Option<&std::path::Path>) -> Option<String> {
    if specifier.starts_with(STD_PREFIX) || specifier.starts_with(CORE_PREFIX) {
        let provider = varn_modules::provider::get()?;
        if let Some(mod_path) = provider.source_path(specifier) {
            if mod_path.is_file() {
                let canonical = std::fs::canonicalize(&mod_path).ok()?;
                return Some(path_to_uri(&canonical.to_string_lossy()));
            }
        }
        if provider.embedded_source(specifier).is_some() {
            return Some(varn_modules::resolver::to_varn_uri(specifier));
        }
        return None;
    }

    if specifier.starts_with('.') {
        let dir = doc_dir?;
        let mut joined = dir.join(specifier);
        varn_modules::resolver::ensure_varn_extension(&mut joined);
        let canonical = std::fs::canonicalize(&joined).ok()?;
        return Some(path_to_uri(&canonical.to_string_lossy()));
    }

    None
}

fn is_indexable(kind: SymbolKind, line: u32) -> bool {
    if line == u32::MAX {
        return false;
    }
    !matches!(kind, SymbolKind::Parameter | SymbolKind::TypeParameter)
}
