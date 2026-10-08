pub use super::uri::{path_to_uri, percent_decode, uri_to_path};
pub use super::{
    canonical_or_original, is_pkg_specifier, normalize_path_string, resolve_pkg_specifier,
    resolve_pkg_specifier_detailed, resolve_specifier_path,
};

use std::path::{Component, Path, PathBuf};
use varn_core::ModuleId;

pub struct ModuleResolver;

impl Default for ModuleResolver {
    fn default() -> Self {
        Self
    }
}

impl ModuleResolver {
    pub fn new() -> Self {
        Self
    }

    pub fn resolve(&self, spec: &str, referrer: &ModuleId) -> Result<ModuleId, String> {
        use varn_core::ImportSpecifier;
        match ImportSpecifier::parse(spec) {
            ImportSpecifier::Stdlib(s) => Ok(ModuleId::Std(s)),
            ImportSpecifier::Core(s) => Ok(ModuleId::Core(s)),
            ImportSpecifier::Runtime(s) => Ok(ModuleId::Runtime(s)),
            ImportSpecifier::Relative(rel) => {
                let joined = if rel.is_absolute() {
                    rel
                } else {
                    let base = match referrer {
                        ModuleId::Local(s) => PathBuf::from(s.as_ref()),
                        ModuleId::Core(_)
                        | ModuleId::Std(_)
                        | ModuleId::Runtime(_)
                        | ModuleId::Package { .. } => PathBuf::from("."),
                    };
                    let base_dir = base.parent().unwrap_or(Path::new("."));
                    base_dir.join(&rel)
                };
                let normalized = normalize_components(&joined);
                let path_str = normalize_path_string(normalized.to_string_lossy().into_owned());
                Ok(ModuleId::local_str(&path_str))
            }
            ImportSpecifier::Package(s) => {
                let base = match referrer {
                    ModuleId::Local(s) => PathBuf::from(s.as_ref()),
                    ModuleId::Core(_)
                    | ModuleId::Std(_)
                    | ModuleId::Runtime(_)
                    | ModuleId::Package { .. } => PathBuf::from("."),
                };
                let base_dir = base.parent().unwrap_or(Path::new("."));
                super::resolve_pkg_specifier(base_dir, s.as_ref())
                    .map(|p| ModuleId::local_str(&p))
                    .ok_or_else(|| format!("cannot resolve package '{spec}'"))
            }
        }
    }
}

fn normalize_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other @ Component::Prefix(_)
            | other @ Component::RootDir
            | other @ Component::Normal(_) => out.push(other),
        }
    }
    out
}

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
        .enumerate()
        .take_while(|(i, (a, b))| {
            if *i == 0 && is_drive_segment(a) && is_drive_segment(b) {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        })
        .count();

    if common == 0 && to_parts.first().is_some_and(|s| is_drive_segment(s)) {
        let mut absolute = to;
        if absolute.ends_with(".vn") {
            absolute.truncate(absolute.len() - 3);
        }
        return absolute;
    }

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

fn is_drive_segment(seg: &str) -> bool {
    let b = seg.as_bytes();
    b.len() == 2 && b[1] == b':' && b[0].is_ascii_alphabetic()
}

pub fn is_known_module(specifier: &str) -> bool {
    super::is_known_stdlib_module(specifier)
}

pub const DOCS_BASE_URL: &str = "https://varn-lang.dev/errors";

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
