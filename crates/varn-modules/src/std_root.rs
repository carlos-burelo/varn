//! Resolution of the "active std": which bundle or source tree serves
//! `std:` imports. One mechanism, two storage forms (spec §4).
//!
//! Order: project varn.json `"std"` override → VARN_STD env (dev/CI) →
//! this checkout's `std/` tree (any binary built under `target/`, however
//! it's launched) → `std.vnb` next to the executable.

use std::path::{Path, PathBuf};

pub const ENV_VARN_STD: &str = "VARN_STD";
pub const STD_MANIFEST_FILE: &str = "std.json";
pub const STD_BUNDLE_FILE: &str = "std.vnb";
pub const STD_DIR_NAME: &str = "std";

#[derive(Debug, Clone)]
pub enum StdSource {
    Bundle(PathBuf),
    SourceTree(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdProvenance {
    ProjectOverride,
    Env,
    /// This checkout's own `std/` tree, found by walking up from the running
    /// binary's location. Covers any launcher (editor, debugger, direct exe)
    /// without needing cargo to inject `VARN_STD` — that only reaches
    /// `cargo run`/`test`, not a subprocess spawned straight off disk.
    DevCheckout,
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

/// The active std is process-fixed (same contract as the provider's
/// `ACTIVE_STD` OnceLock): resolution walks the filesystem once and the
/// result is cached. Callers on hot paths (binder, resolver) may call this
/// freely.
pub fn resolve() -> Option<(StdSource, StdProvenance)> {
    static RESOLVED: std::sync::OnceLock<Option<(StdSource, StdProvenance)>> =
        std::sync::OnceLock::new();
    RESOLVED.get_or_init(resolve_uncached).clone()
}

fn resolve_uncached() -> Option<(StdSource, StdProvenance)> {
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
    if let Some(src) = dev_checkout_std() {
        return Some((src, StdProvenance::DevCheckout));
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

/// True when `file` sits inside the active std source tree (dev mode only;
/// bundle mode is always false). Grants stdlib context — `core:` imports —
/// to files compiled straight from the tree. Canonicalized tree root is
/// cached; per-file verdicts are memoized per thread.
pub fn in_source_tree(file: &str) -> bool {
    static TREE_ROOT: std::sync::OnceLock<Option<(PathBuf, Option<PathBuf>)>> =
        std::sync::OnceLock::new();
    let Some((root, canon_root)) = TREE_ROOT.get_or_init(|| match resolve() {
        Some((StdSource::SourceTree(p), _)) => {
            let canon = std::fs::canonicalize(&p).ok();
            Some((p, canon))
        }
        _ => None,
    }) else {
        return false;
    };

    thread_local! {
        static MEMO: std::cell::RefCell<std::collections::HashMap<Box<str>, bool>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    if let Some(hit) = MEMO.with(|m| m.borrow().get(file).copied()) {
        return hit;
    }
    let path = Path::new(file);
    let verdict = match (std::fs::canonicalize(path).ok(), canon_root) {
        (Some(p), Some(r)) => p.starts_with(r),
        _ => path.starts_with(root),
    };
    MEMO.with(|m| m.borrow_mut().insert(Box::from(file), verdict));
    verdict
}

/// Walks up from the running binary looking for a sibling `std/` tree
/// (`std/std.json`) — the layout of this repo's own `target/<profile>/`.
/// Released binaries ship without a `std/` dir next to them, so this is a
/// no-op there and resolution falls through to the bundle tier.
fn dev_checkout_std() -> Option<StdSource> {
    let exe = std::env::current_exe().ok()?;
    find_dev_std_from(&exe)
}

fn find_dev_std_from(start: &Path) -> Option<StdSource> {
    start
        .ancestors()
        .find_map(|dir| classify(&dir.join(STD_DIR_NAME)))
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
    fn finds_dev_checkout_std_by_walking_up_from_the_binary() {
        let root = std::env::temp_dir().join("varn_stdroot_test_checkout");
        let std_dir = root.join("std");
        let _ = std::fs::create_dir_all(&std_dir);
        std::fs::write(std_dir.join("std.json"), "{}").unwrap();

        // Mirrors this repo's layout: target/<profile>/<exe> sits two levels
        // below the checkout root that also holds `std/`.
        let fake_exe = root.join("target").join("debug").join("vn-lsp.exe");
        let _ = std::fs::create_dir_all(fake_exe.parent().unwrap());

        match find_dev_std_from(&fake_exe) {
            Some(StdSource::SourceTree(p)) => assert_eq!(p, std_dir),
            other => panic!("expected SourceTree({}), got {other:?}", std_dir.display()),
        }
    }

    #[test]
    fn no_dev_checkout_std_when_none_present_up_the_tree() {
        let fake_exe = std::env::temp_dir()
            .join("varn_stdroot_test_no_checkout")
            .join("target")
            .join("debug")
            .join("vn-lsp.exe");
        let _ = std::fs::create_dir_all(fake_exe.parent().unwrap());
        assert!(find_dev_std_from(&fake_exe).is_none());
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
