use std::path::{Path, PathBuf};

pub const ENV_VARN_STD: &str = "VARN_STD";
pub const STD_MANIFEST_FILE: &str = "std.json";
pub const STD_BUNDLE_FILE: &str = "std.vnb";
pub const STD_DIR_NAME: &str = "std";

pub const STD_EMBEDDED_SENTINEL: &str = "@embedded";

#[derive(Debug, Clone)]
pub enum StdSource {
    SourceTree(PathBuf),

    Embedded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdProvenance {
    ProjectOverride,
    Env,

    DevCheckout,

    Embedded,
}

pub fn classify(path: &Path) -> Option<StdSource> {
    (path.is_dir()
        && (path.join(STD_MANIFEST_FILE).is_file() || path.join("math/mod.vn").is_file()))
    .then(|| StdSource::SourceTree(path.to_path_buf()))
}

pub fn project_std_override(project_root: &Path) -> Option<PathBuf> {
    let manifest_path = if project_root
        .join(crate::artifact::PACKAGE_MANIFEST_FILE)
        .exists()
    {
        project_root.join(crate::artifact::PACKAGE_MANIFEST_FILE)
    } else if project_root
        .join(crate::artifact::PACKAGE_MANIFEST_FILE_VN)
        .exists()
    {
        project_root.join(crate::artifact::PACKAGE_MANIFEST_FILE_VN)
    } else {
        return None;
    };
    let raw = std::fs::read_to_string(&manifest_path).ok()?;
    #[derive(serde::Deserialize)]
    struct StdKey {
        std: Option<String>,
    }
    let parsed: StdKey = toml::from_str(&raw).ok()?;
    let rel = parsed.std?;
    let p = PathBuf::from(&rel);
    Some(if p.is_absolute() {
        p
    } else {
        project_root.join(p)
    })
}

pub fn resolve() -> (StdSource, StdProvenance) {
    static RESOLVED: std::sync::OnceLock<(StdSource, StdProvenance)> = std::sync::OnceLock::new();
    RESOLVED.get_or_init(resolve_uncached).clone()
}

fn resolve_uncached() -> (StdSource, StdProvenance) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = crate::artifact::find_project_root(&cwd);
    if let Some(p) = project_std_override(&project_root) {
        if let Some(src) = classify(&p) {
            return (src, StdProvenance::ProjectOverride);
        }
    }
    if let Ok(p) = std::env::var(ENV_VARN_STD) {
        if p == STD_EMBEDDED_SENTINEL {
            return (StdSource::Embedded, StdProvenance::Env);
        }
        if let Some(src) = classify(Path::new(&p)) {
            return (src, StdProvenance::Env);
        }
    }
    if let Some(src) = dev_checkout_std() {
        return (src, StdProvenance::DevCheckout);
    }
    (StdSource::Embedded, StdProvenance::Embedded)
}

pub fn in_source_tree(file: &str) -> bool {
    static TREE_ROOT: std::sync::OnceLock<Option<(PathBuf, Option<PathBuf>)>> =
        std::sync::OnceLock::new();
    let Some((root, canon_root)) = TREE_ROOT.get_or_init(|| match resolve() {
        (StdSource::SourceTree(p), _) => {
            let canon = std::fs::canonicalize(&p).ok();
            Some((p, canon))
        }
        _ => None,
    }) else {
        return false;
    };

    thread_local! {
        static MEMO: std::cell::RefCell<rustc_hash::FxHashMap<Box<str>, bool>> =
            std::cell::RefCell::new(rustc_hash::FxHashMap::default());
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

fn dev_checkout_std() -> Option<StdSource> {
    let exe = std::env::current_exe().ok()?;
    find_dev_std_from(&exe)
}

fn find_dev_std_from(start: &Path) -> Option<StdSource> {
    start
        .ancestors()
        .find_map(|dir| classify(&dir.join(STD_DIR_NAME)))
}
