use varn_checker::SymbolKind;
use varn_modules::spec::{BUILTIN_PREFIX, STD_PREFIX};

use crate::document::{import::uri_to_path, DocumentState, MemberKind, MemberRecord};

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

    // Index exported members with stable keys so cross-file go-to-definition
    // can resolve methods/properties without name heuristics.
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
                kind: member_kind_to_symbol_kind(m.kind),
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

fn member_kind_to_symbol_kind(kind: MemberKind) -> SymbolKind {
    match kind {
        MemberKind::Constructor => SymbolKind::Method,
        MemberKind::Method => SymbolKind::Method,
        MemberKind::Function => SymbolKind::Function,
        MemberKind::Property => SymbolKind::Property,
        MemberKind::Variable => SymbolKind::Var,
        MemberKind::EnumMember => SymbolKind::EnumMember,
        MemberKind::Getter => SymbolKind::Property,
        MemberKind::Setter => SymbolKind::Property,
        MemberKind::Class => SymbolKind::Class,
        MemberKind::Interface => SymbolKind::Interface,
        MemberKind::Namespace => SymbolKind::Namespace,
        MemberKind::Enum => SymbolKind::Enum,
        MemberKind::Struct => SymbolKind::Struct,
    }
}

fn resolve_specifier_to_uri(specifier: &str, doc_dir: Option<&std::path::Path>) -> Option<String> {
    if specifier.starts_with(STD_PREFIX) || specifier.starts_with(BUILTIN_PREFIX) {
        let loader = varn_builtins::BuiltinSourceLocator::from_env();
        if loader.embedded_source(specifier).is_some() {
            return Some(format!("varn-stdlib://{specifier}"));
        }
        let mod_path = loader.vn_source_path(specifier)?;
        if mod_path.is_file() {
            let canonical = std::fs::canonicalize(&mod_path).ok()?;
            return Some(path_to_uri(&canonical.to_string_lossy()));
        }
        return None;
    }

    if specifier.starts_with('.') {
        let dir = doc_dir?;
        let joined = dir.join(specifier);
        let with_ext = if joined.extension().is_none() {
            joined.with_extension("vn")
        } else {
            joined
        };
        let canonical = std::fs::canonicalize(&with_ext).ok()?;
        return Some(path_to_uri(&canonical.to_string_lossy()));
    }

    None
}

fn path_to_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

fn is_indexable(kind: SymbolKind, line: u32) -> bool {
    if line == u32::MAX {
        return false;
    }
    !matches!(kind, SymbolKind::Parameter | SymbolKind::TypeParameter)
}
