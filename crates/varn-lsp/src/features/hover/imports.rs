use std::path::Path;

use crate::constants::STD_PREFIX;
use crate::document::uri_to_path;

use super::make_lang_hover;
use tower_lsp::lsp_types::Hover;

pub fn import_path_hover(specifier: &str, doc_uri: &str) -> Option<Hover> {
    let display = if specifier.starts_with(STD_PREFIX) || is_incomplete_import_specifier(specifier)
    {
        specifier.to_owned()
    } else {
        resolve_import_module_path(specifier, doc_uri)
    };
    Some(make_lang_hover(format!("module \"{display}\"")))
}

fn is_incomplete_import_specifier(specifier: &str) -> bool {
    let trimmed = specifier.trim();
    trimmed.is_empty() || matches!(trimmed, "." | ".." | "./" | "../") || trimmed.ends_with('/')
}

fn resolve_import_module_path(specifier: &str, doc_uri: &str) -> String {
    let doc_path = uri_to_path(doc_uri);
    let doc_dir = match Path::new(&doc_path).parent() {
        Some(dir) => dir.to_path_buf(),
        None => return specifier.to_owned(),
    };

    let joined = doc_dir.join(specifier);
    let with_ext = if joined.extension().is_none() {
        joined.with_extension("vn")
    } else {
        joined
    };

    let mut resolved = std::fs::canonicalize(&with_ext).unwrap_or(with_ext);
    if resolved.extension().and_then(|ext| ext.to_str()) == Some("Varn") {
        resolved.set_extension("");
    }

    normalize_display_path(&resolved.to_string_lossy())
}

fn normalize_display_path(path: &str) -> String {
    let without_verbatim = path
        .strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix("//?/"))
        .unwrap_or(path);
    without_verbatim.replace('\\', "/")
}
