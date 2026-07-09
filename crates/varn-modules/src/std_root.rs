//! Resolution of the "active std": which bundle or source tree serves
//! `std:` imports. One mechanism, two storage forms (spec §4).
//!
//! Order: project varn.json `"std"` override → VARN_STD env (dev/CI) →
//! `std.vnb` next to the executable.

use std::path::{Path, PathBuf};

pub const ENV_VARN_STD: &str = "VARN_STD";
pub const STD_MANIFEST_FILE: &str = "std.json";
pub const STD_BUNDLE_FILE: &str = "std.vnb";

#[derive(Debug, Clone)]
pub enum StdSource {
    Bundle(PathBuf),
    SourceTree(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdProvenance {
    ProjectOverride,
    Env,
    Toolchain,
}

/// A dir with std.json = source tree; a file = bundle.
pub fn classify(path: &Path) -> Option<StdSource> {
    if path.is_dir() && path.join(STD_MANIFEST_FILE).is_file() {
        return Some(StdSource::SourceTree(path.to_path_buf()));
    }
    if path.is_file() {
        return Some(StdSource::Bundle(path.to_path_buf()));
    }
    None
}

/// `"std"` key in the project's varn.json, resolved relative to the manifest.
pub fn project_std_override(project_root: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(
        project_root.join(crate::artifact::PACKAGE_MANIFEST_FILE),
    )
    .ok()?;
    #[derive(serde::Deserialize)]
    struct StdKey {
        std: Option<String>,
    }
    let parsed: StdKey = serde_json::from_str(&raw).ok()?;
    let rel = parsed.std?;
    let p = PathBuf::from(&rel);
    Some(if p.is_absolute() { p } else { project_root.join(p) })
}

pub fn resolve() -> Option<(StdSource, StdProvenance)> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = crate::artifact::find_project_root(&cwd);
    if let Some(p) = project_std_override(&project_root) {
        if let Some(src) = classify(&p) {
            return Some((src, StdProvenance::ProjectOverride));
        }
    }
    if let Ok(p) = std::env::var(ENV_VARN_STD) {
        if let Some(src) = classify(Path::new(&p)) {
            return Some((src, StdProvenance::Env));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(src) = classify(&dir.join(STD_BUNDLE_FILE)) {
                return Some((src, StdProvenance::Toolchain));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_bundle_vs_tree() {
        let dir = std::env::temp_dir().join("varn_stdroot_test_tree");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("std.json"), "{}").unwrap();
        assert!(matches!(classify(&dir), Some(StdSource::SourceTree(_))));

        let f = std::env::temp_dir().join("varn_stdroot_test.vnb");
        std::fs::write(&f, b"VNB\0").unwrap();
        assert!(matches!(classify(&f), Some(StdSource::Bundle(_))));

        let missing = std::env::temp_dir().join("varn_stdroot_missing_xyz");
        assert!(classify(&missing).is_none());
    }

    #[test]
    fn reads_project_override() {
        let dir = std::env::temp_dir().join("varn_stdroot_test_proj");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("varn.json"), r#"{"std": "custom/std.vnb"}"#).unwrap();
        let p = project_std_override(&dir).unwrap();
        assert!(p.ends_with("custom/std.vnb"));

        std::fs::write(dir.join("varn.json"), r#"{"name": "x"}"#).unwrap();
        assert!(project_std_override(&dir).is_none());
    }
}
