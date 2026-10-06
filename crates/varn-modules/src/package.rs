use super::ids::{
    DEFAULT_PACKAGE_VERSION, ENV_DIR_NAME, MODULES_DIR_NAME, PACKAGE_MANIFEST_FILE,
    PACKAGE_MANIFEST_FILE_VN, PKG_PREFIX, RELATIVE_EXPORT_PREFIX,
};
use semver::{Version, VersionReq};
use std::path::{Path, PathBuf};

pub fn is_pkg_specifier(specifier: &str) -> bool {
    specifier.starts_with(PKG_PREFIX)
        || (!specifier.starts_with('.')
            && !specifier.starts_with('/')
            && !specifier.starts_with('\\')
            && !specifier.contains(':'))
}

pub fn split_pkg_specifier(specifier: &str) -> Option<(String, Option<String>)> {
    let raw = specifier.strip_prefix(PKG_PREFIX).unwrap_or(specifier);
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

    if let Some(target) = manifest.exports.get(&export_key) {
        let entry = package_root.join(target.trim_start_matches(RELATIVE_EXPORT_PREFIX));
        if entry.exists() {
            return Ok(entry);
        }
    }

    for (key, val) in &manifest.exports {
        if key.contains('*') {
            let prefix = key.trim_end_matches('*');
            if let Some(matched_suffix) = export_key.strip_prefix(prefix) {
                let resolved_val = val.replace('*', matched_suffix);
                let entry =
                    package_root.join(resolved_val.trim_start_matches(RELATIVE_EXPORT_PREFIX));
                if entry.exists() {
                    return Ok(entry);
                }
            }
        }
    }

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
        manifest.name.as_deref().unwrap_or(package_name),
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

        if let Ok(manifest) = load_package_manifest(dir) {
            if manifest.name.as_deref() == Some(package_name) {
                return Some(dir.to_path_buf());
            }
        }
    }
    None
}

#[derive(serde::Deserialize, Default)]
struct PackageSection {
    name: Option<String>,
    version: Option<String>,
    main: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct RawManifest {
    #[serde(default)]
    package: Option<PackageSection>,
    #[serde(default)]
    project: Option<PackageSection>,

    name: Option<String>,
    version: Option<String>,
    main: Option<String>,

    #[serde(default)]
    exports: rustc_hash::FxHashMap<String, String>,
    #[serde(default)]
    dependencies: rustc_hash::FxHashMap<String, String>,
    #[serde(default, alias = "dev-dependencies", alias = "dev_dependencies")]
    dev_dependencies: rustc_hash::FxHashMap<String, String>,
    #[serde(default, alias = "peer-dependencies", alias = "peer_dependencies")]
    peer_dependencies: rustc_hash::FxHashMap<String, String>,
    #[serde(default)]
    workspaces: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PackageManifest {
    pub name: Option<String>,
    pub version: Option<String>,
    pub main: Option<String>,
    pub exports: rustc_hash::FxHashMap<String, String>,
    pub dependencies: rustc_hash::FxHashMap<String, String>,
    pub dev_dependencies: rustc_hash::FxHashMap<String, String>,
    pub peer_dependencies: rustc_hash::FxHashMap<String, String>,
    pub workspaces: Vec<String>,
}

fn load_package_manifest(package_root: &Path) -> Result<PackageManifest, String> {
    let manifest_path = if package_root.join(PACKAGE_MANIFEST_FILE).exists() {
        package_root.join(PACKAGE_MANIFEST_FILE)
    } else if package_root.join(PACKAGE_MANIFEST_FILE_VN).exists() {
        package_root.join(PACKAGE_MANIFEST_FILE_VN)
    } else {
        return Err(format!(
            "missing {} in package root {}",
            PACKAGE_MANIFEST_FILE,
            package_root.display()
        ));
    };

    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let parsed: RawManifest =
        toml::from_str(&raw).map_err(|e| format!("invalid {}: {e}", manifest_path.display()))?;

    let name = parsed
        .package
        .as_ref()
        .and_then(|p| p.name.clone())
        .or_else(|| parsed.project.as_ref().and_then(|p| p.name.clone()))
        .or(parsed.name);
    let version = parsed
        .package
        .as_ref()
        .and_then(|p| p.version.clone())
        .or_else(|| parsed.project.as_ref().and_then(|p| p.version.clone()))
        .or(parsed.version);
    let main = parsed
        .package
        .as_ref()
        .and_then(|p| p.main.clone())
        .or_else(|| parsed.project.as_ref().and_then(|p| p.main.clone()))
        .or(parsed.main);

    Ok(PackageManifest {
        name,
        version,
        main,
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
        path.replace('\\', "/")
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

    let req_str = if let Some((_, semver_part)) = required.rsplit_once('@') {
        semver_part
    } else {
        required.as_str()
    };

    if required.starts_with("path:") || req_str == "*" {
        return Ok(());
    }

    let req = VersionReq::parse(req_str).map_err(|e| {
        format!(
            "invalid semver constraint '{}' for dependency '{}': {}",
            req_str, package_name, e
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
            package_name, req_str, resolved_version
        ));
    }
    Ok(())
}

fn nearest_owner_manifest(base_dir: &Path) -> Option<PackageManifest> {
    for dir in base_dir.ancestors() {
        if dir.ends_with(Path::new(ENV_DIR_NAME).join(MODULES_DIR_NAME)) {
            continue;
        }
        if let Ok(manifest) = load_package_manifest(dir) {
            return Some(manifest);
        }
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
