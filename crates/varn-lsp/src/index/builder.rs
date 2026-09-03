use crate::document::SymbolView;
use varn_checker::SymbolKind;
use varn_modules::resolver::path_to_uri;
use varn_modules::spec::{CORE_PREFIX, STD_PREFIX};

use crate::document::{import::uri_to_path, DocumentState};

use super::{ExportEntry, ProjectIndex};

pub fn index_file(index: &mut ProjectIndex, uri: &str, state: &DocumentState) {
    let mut exports: Vec<ExportEntry> = state
        .symbols()
        .filter(|s| is_indexable(s.kind(), s.line()))
        .map(|s| ExportEntry {
            name: s.name().to_owned(),
            global_key: s.global_key(true),
            kind: s.kind(),
            uri: uri.to_owned(),
            line: s.line(),
            col: s.col(),
            type_str: s.type_str(),
            doc: s.doc().map(str::to_owned),
        })
        .collect();

    for sym in state.symbols().filter(|s| is_indexable(s.kind(), s.line())) {
        collect_member_exports(state, uri, sym, &mut exports);
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

/// Index the members `sym` declares, so a cross-file lookup can reach them.
///
/// Asked of the checker rather than read from a mirrored member tree. Only
/// members the checker located are indexed: one with no `def_line` has no
/// source of its own, and an index entry pointing nowhere is worse than none.
fn collect_member_exports(
    state: &crate::document::DocumentState,
    uri: &str,
    sym: SymbolView<'_>,
    out: &mut Vec<ExportEntry>,
) {
    for m in state.members_of(sym) {
        let Some(line) = m.def_line else { continue };
        out.push(ExportEntry {
            name: m.name.to_string(),
            global_key: format!("member:{}:{}", sym.name(), m.name),
            kind: summary_to_symbol_kind(m.kind),
            uri: uri.to_owned(),
            line: line.saturating_sub(1),
            col: m.def_col,
            type_str: m.ty.to_string(),
            doc: None,
        });
    }
}

fn summary_to_symbol_kind(k: varn_checker::ResolvedMemberKind) -> varn_checker::SymbolKind {
    use varn_checker::ResolvedMemberKind as R;
    use varn_checker::SymbolKind as S;
    match k {
        R::Method | R::StaticMethod | R::ExtensionMethod => S::Method,
        R::EnumMember => S::EnumMember,
        _ => S::Property,
    }
}

fn resolve_specifier_to_uri(specifier: &str, doc_dir: Option<&std::path::Path>) -> Option<String> {
    if specifier.starts_with(STD_PREFIX) || specifier.starts_with(CORE_PREFIX) {
        let path = crate::workspace::std_sources::resolve_module_file(specifier)?;
        return Some(path_to_uri(&path.to_string_lossy()));
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
