pub mod spec;

use semver::{Version, VersionReq};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const BUILTIN_GLOBAL: &str = "builtin:global";
pub const BUILTIN_CLASSES: &str = "builtin:classes";
pub const BUILTIN_ERRORS: &str = "builtin:errors";
pub const BUILTIN_COLLECTIONS: &str = "builtin:collections";
pub const BUILTIN_ASYNC: &str = "builtin:async";
pub const BUILTIN_ITERATORS: &str = "builtin:iterators";
pub const BUILTIN_REFLECT: &str = "builtin:reflect";

pub const PKG_PREFIX: &str = "pkg:";
pub const EMBEDDED_MODULE_PREFIX: &str = "varn-embedded:";
pub const ENV_DIR_NAME: &str = ".vn";
pub const MODULES_DIR_NAME: &str = "packages";
pub const PACKAGE_MANIFEST_FILE: &str = "varn.json";
pub const VARN_FILE_EXTENSION: &str = "vn";
pub const VARN_INDEX_FILE: &str = "index.vn";
pub const DEFAULT_PACKAGE_VERSION: &str = "0.0.0";
pub const RELATIVE_EXPORT_PREFIX: &str = "./";

pub const BUILTIN_STR: &str = "builtin:str";
pub const BUILTIN_INT: &str = "builtin:int";
pub const BUILTIN_FLOAT: &str = "builtin:float";
pub const BUILTIN_BOOL: &str = "builtin:bool";
pub const BUILTIN_CHAR: &str = "builtin:char";
pub const BUILTIN_DECIMAL: &str = "builtin:decimal";
pub const BUILTIN_RANGE: &str = "builtin:range";
pub const BUILTIN_ARRAY: &str = "builtin:array";

pub const STD_TASK: &str = "std:task";
pub const STD_COLLECTIONS: &str = "std:collections";
pub const STD_CORE: &str = "std:core";
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
pub const STD_RESULT: &str = "std:result";
pub const STD_OPTION: &str = "std:option";
pub const STD_SYS: &str = "std:sys";
pub const STD_TEST: &str = "std:test";
pub const STD_TIME: &str = "std:time";
pub const STD_TYPES: &str = "std:types";

pub const BUILTIN_MODULES: &[&str] = &[
    BUILTIN_GLOBAL,
    BUILTIN_CLASSES,
    BUILTIN_ERRORS,
    BUILTIN_COLLECTIONS,
    BUILTIN_ASYNC,
    BUILTIN_ITERATORS,
    BUILTIN_REFLECT,
    BUILTIN_STR,
    BUILTIN_INT,
    BUILTIN_FLOAT,
    BUILTIN_BOOL,
    BUILTIN_CHAR,
    BUILTIN_DECIMAL,
    BUILTIN_RANGE,
    BUILTIN_ARRAY,
];

pub const STD_MODULES: &[&str] = &[
    STD_TASK,
    STD_COLLECTIONS,
    STD_CORE,
    STD_CRYPTO,
    STD_DISPOSE,
    STD_FS,
    STD_HTTP,
    STD_IO,
    STD_JSON,
    STD_MATH,
    STD_NET,
    STD_OPTION,
    STD_PATH,
    STD_REFLECT,
    STD_RESULT,
    STD_SYS,
    STD_TEST,
    STD_TIME,
    STD_TYPES,
];

pub use spec::{ModuleKind, ModuleSpec};

pub fn is_embedded_module_path(path: &str) -> bool {
    path.starts_with(EMBEDDED_MODULE_PREFIX)
}

pub fn is_package_module_path(path: &str) -> bool {
    let as_path = Path::new(path);
    let mut prev = None;
    for component in as_path.components() {
        let current = component.as_os_str();
        if prev == Some(OsStr::new(ENV_DIR_NAME)) && current == OsStr::new(MODULES_DIR_NAME) {
            return true;
        }
        prev = Some(current);
    }
    false
}

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
        .unwrap_or_else(|| DEFAULT_PACKAGE_VERSION.to_owned());
    enforce_dependency_constraint(base_dir, &package_name, &version)?;
    let sub = subpath.unwrap_or_default();
    let export_key = if sub.is_empty() {
        ".".to_owned()
    } else {
        format!("./{sub}")
    };
    let export_target = manifest.exports.get(&export_key).cloned().ok_or_else(|| {
        format!(
            "package '{}' does not export '{}'",
            package_name, export_key
        )
    })?;

    let entry = package_root.join(export_target.trim_start_matches(RELATIVE_EXPORT_PREFIX));
    let resolved = resolve_path_candidates(&entry).ok_or_else(|| {
        format!(
            "export target not found for '{}': {}",
            specifier,
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

fn find_package_root(base_dir: &Path, package_name: &str) -> Option<PathBuf> {
    for dir in base_dir.ancestors() {
        let env_modules = dir
            .join(ENV_DIR_NAME)
            .join(MODULES_DIR_NAME)
            .join(package_name);
        if env_modules.exists() {
            return Some(env_modules);
        }
    }
    None
}

#[derive(serde::Deserialize)]
struct RawManifest {
    version: Option<String>,
    #[serde(default)]
    exports: std::collections::HashMap<String, String>,
    #[serde(default)]
    dependencies: std::collections::HashMap<String, String>,
}

struct PackageManifest {
    version: Option<String>,
    exports: std::collections::HashMap<String, String>,
    dependencies: std::collections::HashMap<String, String>,
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
    if parsed.exports.is_empty() {
        return Err(format!(
            "package manifest {} must declare non-empty 'exports'",
            manifest_path.display()
        ));
    }

    Ok(PackageManifest {
        version: parsed.version,
        exports: parsed.exports,
        dependencies: parsed.dependencies,
    })
}

fn resolve_path_candidates(target: &Path) -> Option<String> {
    let mut candidates = Vec::new();
    if target.extension().is_some() {
        candidates.push(target.to_path_buf());
    } else {
        candidates.push(target.with_extension(VARN_FILE_EXTENSION));
        candidates.push(target.join(VARN_INDEX_FILE));
        candidates.push(target.to_path_buf());
    }

    for candidate in candidates {
        if let Some(resolved) = canonical_or_string(&candidate) {
            return Some(resolved);
        }
    }
    None
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

fn normalize_path_string(path: String) -> String {
    #[cfg(windows)]
    {
        if let Some(rest) = path.strip_prefix("\\\\?\\") {
            return rest.to_owned();
        }
    }
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
            version: parsed.version,
            exports: parsed.exports,
            dependencies: parsed.dependencies,
        });
    }
    None
}

pub fn varn_source_for(specifier: &str) -> Option<&'static str> {
    Some(match specifier {
        BUILTIN_GLOBAL => "crates/varn-builtins/src/modules/globals/globals.vn",
        BUILTIN_CLASSES => "crates/varn-builtins/src/modules/globals/classes.vn",
        BUILTIN_ERRORS => "crates/varn-builtins/src/modules/globals/errors.vn",
        BUILTIN_COLLECTIONS => "crates/varn-builtins/src/modules/globals/collections.vn",
        BUILTIN_ASYNC => "crates/varn-builtins/src/modules/globals/async.vn",
        BUILTIN_ITERATORS => "crates/varn-builtins/src/modules/globals/iterators.vn",
        BUILTIN_REFLECT => "crates/varn-builtins/src/modules/globals/reflect.vn",
        BUILTIN_STR => "crates/varn-builtins/src/modules/primitives/str/str.vn",
        BUILTIN_INT => "crates/varn-builtins/src/modules/primitives/int/int.vn",
        BUILTIN_FLOAT => "crates/varn-builtins/src/modules/primitives/float/float.vn",
        BUILTIN_BOOL => "crates/varn-builtins/src/modules/primitives/boolean/boolean.vn",
        BUILTIN_CHAR => "crates/varn-builtins/src/modules/primitives/char/char.vn",
        BUILTIN_DECIMAL => "crates/varn-builtins/src/modules/primitives/decimal/decimal.vn",
        BUILTIN_RANGE => "crates/varn-builtins/src/modules/primitives/range/range.vn",
        BUILTIN_ARRAY => "crates/varn-builtins/src/modules/primitives/array/array.vn",
        STD_TASK => "crates/varn-builtins/src/modules/std/task/task.vn",
        STD_COLLECTIONS => "crates/varn-builtins/src/modules/std/collections/collections.vn",
        STD_CORE => "crates/varn-builtins/src/modules/std/core/core.vn",
        STD_CRYPTO => "crates/varn-builtins/src/modules/std/crypto/crypto.vn",
        STD_DISPOSE => "crates/varn-builtins/src/modules/globals/dispose.vn",
        STD_FS => "crates/varn-builtins/src/modules/std/fs/fs.vn",
        STD_HTTP => "crates/varn-builtins/src/modules/std/http/http.vn",
        STD_IO => "crates/varn-builtins/src/modules/std/io/io.vn",
        STD_JSON => "crates/varn-builtins/src/modules/std/json/json.vn",
        STD_MATH => "crates/varn-builtins/src/modules/std/math/math.vn",
        STD_NET => "crates/varn-builtins/src/modules/std/net/net.vn",
        STD_OPTION => "crates/varn-builtins/src/modules/std/option/option.vn",
        STD_PATH => "crates/varn-builtins/src/modules/std/path/path.vn",
        STD_REFLECT => "crates/varn-builtins/src/modules/std/reflect/reflect.vn",
        STD_RESULT => "crates/varn-builtins/src/modules/std/result/result.vn",
        STD_SYS => "crates/varn-builtins/src/modules/std/sys/sys.vn",
        STD_TEST => "crates/varn-builtins/src/modules/std/testing/testing.vn",
        STD_TIME => "crates/varn-builtins/src/modules/std/time/time.vn",
        STD_TYPES => "crates/varn-builtins/src/modules/std/types/types.vn",
        _ => return None,
    })
}
