pub mod artifact;
pub mod bundle;
pub mod provider;
pub mod resolver;
pub mod spec;
pub mod std_root;
pub mod uri;

use semver::{Version, VersionReq};
use std::path::{Path, PathBuf};

pub const CORE_GLOBAL: &str = "core:global";
pub const CORE_BIGINT: &str = "core:bigint";
pub const CORE_MAP: &str = "core:map";
pub const CORE_SET: &str = "core:set";
pub const CORE_SYMBOL: &str = "core:symbol";

pub const PKG_PREFIX: &str = "pkg:";
pub const ENV_DIR_NAME: &str = ".vn";
pub const MODULES_DIR_NAME: &str = "packages";
pub const PACKAGE_MANIFEST_FILE: &str = "varn.json";
pub const VARN_FILE_EXTENSION: &str = "vn";
pub const DEFAULT_PACKAGE_VERSION: &str = "0.0.0";
pub const RELATIVE_EXPORT_PREFIX: &str = "./";

pub const CORE_STR: &str = "core:str";
pub const CORE_INT: &str = "core:int";
pub const CORE_FLOAT: &str = "core:float";
pub const CORE_BOOL: &str = "core:bool";
pub const CORE_CHAR: &str = "core:char";
pub const CORE_DECIMAL: &str = "core:decimal";
pub const CORE_RANGE: &str = "core:range";
pub const CORE_ARRAY: &str = "core:array";

pub const STD_TASK: &str = "std:task";
pub const STD_COLLECTIONS: &str = "std:collections";
pub const STD_CRYPTO: &str = "std:crypto";
pub const STD_DISPOSE: &str = "std:dispose";
pub const STD_FS: &str = "std:fs";
pub const STD_HTTP: &str = "std:http";
pub const STD_IO: &str = "std:io";
pub const STD_JSON: &str = "std:json";
pub const STD_MATH: &str = "std:math";
pub const STD_NET: &str = "std:net";
pub const STD_PATH: &str = "std:path";
pub const STD_REFLECT: &str = "std:reflect";
pub const STD_SYS: &str = "std:sys";
pub const STD_TEST: &str = "std:test";
pub const STD_TIME: &str = "std:time";
pub const STD_TYPES: &str = "std:types";

pub const RUNTIME_PREFIX: &str = "runtime:";
pub const RUNTIME_FS: &str = "runtime:fs";
pub const RUNTIME_IO: &str = "runtime:io";
pub const RUNTIME_TIME: &str = "runtime:time";
pub const RUNTIME_NET: &str = "runtime:net";
pub const RUNTIME_SYS: &str = "runtime:sys";
pub const RUNTIME_CRYPTO: &str = "runtime:crypto";
pub const RUNTIME_TASK: &str = "runtime:task";
pub const RUNTIME_HTTP: &str = "runtime:http";
pub const RUNTIME_REFLECT: &str = "runtime:reflect";
pub const RUNTIME_JSON: &str = "runtime:json";
pub const RUNTIME_PATH: &str = "runtime:path";
pub const RUNTIME_TESTING: &str = "runtime:testing";

fn module_ids_of_kind(kind: ModuleKind) -> Vec<&'static str> {
    provider::get()
        .map(|p| {
            p.all_specs()
                .iter()
                .filter(|m| m.kind == kind)
                .map(|m| m.id)
                .collect()
        })
        .unwrap_or_default()
}

pub fn core_module_ids() -> Vec<&'static str> {
    module_ids_of_kind(ModuleKind::Core)
}

pub fn std_module_ids() -> Vec<&'static str> {
    module_ids_of_kind(ModuleKind::Stdlib)
}

pub fn is_known_stdlib_module(specifier: &str) -> bool {
    provider::get()
        .and_then(|p| p.spec_for(specifier))
        .is_some()
}

pub use spec::{ModuleKind, ModuleSpec};

pub fn is_pkg_specifier(specifier: &str) -> bool {
    specifier.starts_with(PKG_PREFIX)
}

pub fn split_pkg_specifier(specifier: &str) -> Option<(String, Option<String>)> {
    let raw = specifier.strip_prefix(PKG_PREFIX)?;
    if raw.is_empty() {
        return None;
    }

    if let Some(rest) = raw.strip_prefix('@') {
        let mut parts = rest.splitn(3, '/');
        let scope = parts.next()?;
        let name = parts.next()?;
        if scope.is_empty() || name.is_empty() {
            return None;
        }
        let package = format!("@{scope}/{name}");
        let subpath = parts
            .next()
            .map(|s| s.trim_matches('/').to_owned())
            .filter(|s| !s.is_empty());
        return Some((package, subpath));
    }

    let mut parts = raw.splitn(2, '/');
    let package = parts.next()?.trim();
    if package.is_empty() {
        return None;
    }
    let subpath = parts
        .next()
        .map(|s| s.trim_matches('/').to_owned())
        .filter(|s| !s.is_empty());
    Some((package.to_owned(), subpath))
}

pub fn resolve_pkg_specifier(base_dir: &Path, specifier: &str) -> Option<String> {
    resolve_pkg_specifier_detailed(base_dir, specifier)
        .ok()
        .map(|r| r.resolved_path)
}

#[derive(Clone, Debug)]
pub struct PackageResolution {
    pub specifier: String,
    pub package: String,
    pub version: String,
    pub subpath: String,
    pub package_root: String,
    pub resolved_path: String,
}

pub fn resolve_pkg_specifier_detailed(
    base_dir: &Path,
    specifier: &str,
) -> Result<PackageResolution, String> {
    let (package_name, subpath) = split_pkg_specifier(specifier)
        .ok_or_else(|| format!("invalid package specifier '{specifier}'"))?;
    let package_root = find_package_root(base_dir, &package_name).ok_or_else(|| {
        format!(
            "package '{package_name}' not found from {}",
            base_dir.display()
        )
    })?;

    let manifest = load_package_manifest(&package_root)?;
    let version = manifest
        .version
        .clone()
        .unwrap_or_else(|| DEFAULT_PACKAGE_VERSION.to_owned());
    enforce_dependency_constraint(base_dir, &package_name, &version)?;
    let sub = subpath.unwrap_or_default();

    let entry = resolve_export_target(&package_root, &manifest, &package_name, &sub)?;
    let resolved = resolve_path_candidates(&entry).ok_or_else(|| {
        format!(
            "export target not found for '{specifier}': {}",
            entry.display()
        )
    })?;

    Ok(PackageResolution {
        specifier: specifier.to_owned(),
        package: package_name,
        version,
        subpath: sub,
        package_root: canonical_or_string(&package_root)
            .unwrap_or_else(|| package_root.to_string_lossy().into_owned()),
        resolved_path: resolved,
    })
}

fn resolve_export_target(
    package_root: &Path,
    manifest: &PackageManifest,
    package_name: &str,
    sub: &str,
) -> Result<PathBuf, String> {
    let export_key = if sub.is_empty() {
        ".".to_owned()
    } else {
        format!("./{sub}")
    };

    // 1. Exact match in exports
    if let Some(target) = manifest.exports.get(&export_key) {
        let entry = package_root.join(target.trim_start_matches(RELATIVE_EXPORT_PREFIX));
        if entry.exists() {
            return Ok(entry);
        }
    }

    // 2. Wildcard pattern matching in exports (e.g. "./*" -> "./src/*.vn")
    for (key, val) in &manifest.exports {
        if key.contains('*') {
            let prefix = key.trim_end_matches('*');
            if export_key.starts_with(prefix) {
                let matched_suffix = &export_key[prefix.len()..];
                let resolved_val = val.replace('*', matched_suffix);
                let entry =
                    package_root.join(resolved_val.trim_start_matches(RELATIVE_EXPORT_PREFIX));
                if entry.exists() {
                    return Ok(entry);
                }
            }
        }
    }

    // 3. Fallback convention resolution
    if sub.is_empty() {
        if let Some(ref main_field) = manifest.main {
            let main_path =
                package_root.join(main_field.trim_start_matches(RELATIVE_EXPORT_PREFIX));
            if main_path.exists() {
                return Ok(main_path);
            }
        }
        for candidate_name in &["index.vn", "main.vn", "src/index.vn", "src/main.vn"] {
            let cand = package_root.join(candidate_name);
            if cand.exists() {
                return Ok(cand);
            }
        }
    } else {
        let sub_with_ext = if sub.ends_with(".vn") {
            sub.to_owned()
        } else {
            format!("{sub}.vn")
        };
        for candidate_rel in &[
            sub,
            &sub_with_ext,
            &format!("src/{sub_with_ext}"),
            &format!("{sub}/index.vn"),
        ] {
            let cand = package_root.join(candidate_rel);
            if cand.exists() {
                return Ok(cand);
            }
        }
    }

    Err(format!(
        "cannot resolve subpath '{sub}' in package '{}' (root: {})",
        manifest.name.as_deref().unwrap_or(&package_name),
        package_root.display()
    ))
}

fn find_package_root(base_dir: &Path, package_name: &str) -> Option<PathBuf> {
    for dir in base_dir.ancestors() {
        let env_modules = dir
            .join(ENV_DIR_NAME)
            .join(MODULES_DIR_NAME)
            .join(package_name);
        if env_modules.exists() {
            return Some(env_modules);
        }

        // Check if dir itself is the requested package
        let manifest_file = dir.join(PACKAGE_MANIFEST_FILE);
        if manifest_file.exists() {
            if let Ok(manifest) = load_package_manifest(dir) {
                if manifest.name.as_deref() == Some(package_name) {
                    return Some(dir.to_path_buf());
                }
            }
        }
    }
    None
}

#[derive(serde::Deserialize)]
struct RawManifest {
    name: Option<String>,
    version: Option<String>,
    main: Option<String>,
    #[serde(default)]
    exports: std::collections::HashMap<String, String>,
    #[serde(default)]
    dependencies: std::collections::HashMap<String, String>,
    #[serde(default)]
    dev_dependencies: std::collections::HashMap<String, String>,
    #[serde(default)]
    peer_dependencies: std::collections::HashMap<String, String>,
    #[serde(default)]
    workspaces: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PackageManifest {
    pub name: Option<String>,
    pub version: Option<String>,
    pub main: Option<String>,
    pub exports: std::collections::HashMap<String, String>,
    pub dependencies: std::collections::HashMap<String, String>,
    pub dev_dependencies: std::collections::HashMap<String, String>,
    pub peer_dependencies: std::collections::HashMap<String, String>,
    pub workspaces: Vec<String>,
}

fn load_package_manifest(package_root: &Path) -> Result<PackageManifest, String> {
    let manifest_path = package_root.join(PACKAGE_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(format!(
            "missing {} in package root {}",
            PACKAGE_MANIFEST_FILE,
            package_root.display()
        ));
    }

    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let parsed: RawManifest = serde_json::from_str(&raw)
        .map_err(|e| format!("invalid {}: {e}", manifest_path.display()))?;

    Ok(PackageManifest {
        name: parsed.name,
        version: parsed.version,
        main: parsed.main,
        exports: parsed.exports,
        dependencies: parsed.dependencies,
        dev_dependencies: parsed.dev_dependencies,
        peer_dependencies: parsed.peer_dependencies,
        workspaces: parsed.workspaces,
    })
}

pub(crate) fn resolve_path_candidates(target: &Path) -> Option<String> {
    canonical_or_string(target)
}

fn canonical_or_string(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Some(normalize_path_string(
            canonical.to_string_lossy().into_owned(),
        ));
    }
    Some(normalize_path_string(path.to_string_lossy().into_owned()))
}

pub fn normalize_path_string(path: String) -> String {
    #[cfg(windows)]
    {
        if let Some(rest) = path.strip_prefix("\\\\?\\") {
            return rest.replace('\\', "/");
        }
        return path.replace('\\', "/");
    }
    #[cfg(not(windows))]
    path
}

fn enforce_dependency_constraint(
    base_dir: &Path,
    package_name: &str,
    resolved_version: &str,
) -> Result<(), String> {
    let Some(owner_manifest) = nearest_owner_manifest(base_dir) else {
        return Ok(());
    };
    let Some(required) = owner_manifest.dependencies.get(package_name) else {
        return Ok(());
    };

    if required.starts_with("path:") || required == "*" {
        return Ok(());
    }

    let req = VersionReq::parse(required).map_err(|e| {
        format!(
            "invalid semver constraint '{}' for dependency '{}': {}",
            required, package_name, e
        )
    })?;
    let resolved = Version::parse(resolved_version).map_err(|e| {
        format!(
            "invalid resolved version '{}' for dependency '{}': {}",
            resolved_version, package_name, e
        )
    })?;
    if !req.matches(&resolved) {
        return Err(format!(
            "dependency constraint mismatch for '{}': requires '{}', resolved '{}'",
            package_name, required, resolved_version
        ));
    }
    Ok(())
}

fn nearest_owner_manifest(base_dir: &Path) -> Option<PackageManifest> {
    for dir in base_dir.ancestors() {
        if dir.ends_with(Path::new(ENV_DIR_NAME).join(MODULES_DIR_NAME)) {
            continue;
        }
        let manifest = dir.join(PACKAGE_MANIFEST_FILE);
        if !manifest.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&manifest).ok()?;
        let parsed: RawManifest = serde_json::from_str(&raw).ok()?;
        return Some(PackageManifest {
            name: parsed.name,
            version: parsed.version,
            main: parsed.main,
            exports: parsed.exports,
            dependencies: parsed.dependencies,
            dev_dependencies: parsed.dev_dependencies,
            peer_dependencies: parsed.peer_dependencies,
            workspaces: parsed.workspaces,
        });
    }
    None
}

pub fn canonical_or_original(path: &Path) -> String {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return normalize_path_string(canonical.to_string_lossy().into_owned());
    }
    normalize_path_string(path.to_string_lossy().into_owned())
}

pub fn resolve_specifier_path(base_dir: &Path, specifier: &str) -> Option<String> {
    let target = base_dir.join(specifier);
    resolve_path_candidates(&target)
}
