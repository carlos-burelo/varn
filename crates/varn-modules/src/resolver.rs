pub use super::uri::{path_to_uri, percent_decode, uri_to_path};
pub use super::{
    canonical_or_original, is_pkg_specifier, normalize_path_string, resolve_pkg_specifier,
    resolve_pkg_specifier_detailed, resolve_specifier_path,
};

pub fn normalize_display_path(path: &str) -> String {
    let without_prefix = path
        .strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix("//?/"))
        .unwrap_or(path);
    without_prefix.replace('\\', "/")
}

pub fn relative_import_path(from_file: &str, to_file: &str) -> String {
    let from = normalize_display_path(from_file);
    let to = normalize_display_path(to_file);

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

    if result.ends_with(".vn") {
        result.truncate(result.len() - 3);
    }

    result
}

pub fn is_known_module(specifier: &str) -> bool {
    super::CORE_MODULES.contains(&specifier) || super::STD_MODULES.contains(&specifier)
}

pub const VARN_SCHEME: &str = "varn://";

pub fn to_varn_uri(specifier: &str) -> String {
    // "core:global" → "varn://core/global"
    // "std:io"      → "varn://std/io"
    if let Some((cat, rest)) = specifier.split_once(':') {
        format!("varn://{cat}/{rest}")
    } else {
        format!("varn://{specifier}")
    }
}

pub fn is_varn_uri(uri: &str) -> bool {
    uri.starts_with("varn://")
}

pub fn specifier_from_uri(uri: &str) -> Option<String> {
    let path = uri.strip_prefix("varn://")?;
    let slash = path.find('/')?;
    let cat = &path[..slash];
    let rest = &path[slash + 1..];
    Some(format!("{cat}:{rest}"))
}

pub fn is_known_varn_uri(uri: &str) -> bool {
    specifier_from_uri(uri).is_some_and(|s| is_known_module(&s))
}

pub const DOCS_BASE_URL: &str = "https://varn-lang.dev/errors";

pub fn docs_error_url(error_name: &str) -> String {
    format!("{DOCS_BASE_URL}/{error_name}")
}

pub fn forge_tarball_url(host: &str, user: &str, repo: &str, version: &str) -> String {
    format!("https://{host}/{user}/{repo}/archive/refs/tags/v{version}.tar.gz")
}

pub fn forge_tags_api_url(host: &str, user: &str, repo: &str) -> String {
    if host == "github.com" {
        format!("https://api.github.com/repos/{user}/{repo}/tags")
    } else {
        format!("https://{host}/api/v1/repos/{user}/{repo}/tags")
    }
}

pub fn is_varn_file(path: &str) -> bool {
    path.ends_with(".vn")
}

pub fn ensure_varn_extension(path: &mut std::path::PathBuf) {
    if path.extension().is_none() {
        path.set_extension(super::VARN_FILE_EXTENSION);
    }
}
