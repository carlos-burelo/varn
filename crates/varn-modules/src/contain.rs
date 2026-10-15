use std::path::{Path, PathBuf};

fn canonicalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn find_root_from(base_canon: &Path) -> (PathBuf, bool) {
    for dir in base_canon.ancestors() {
        if dir.join(super::artifact::PACKAGE_MANIFEST_FILE).exists()
            || dir.join(super::artifact::PACKAGE_MANIFEST_FILE_VN).exists()
        {
            return (canonicalize(dir), true);
        }
    }
    (base_canon.to_path_buf(), false)
}

fn root_cache() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, (PathBuf, bool)>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, (PathBuf, bool)>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn project_root_cached(base_dir: &Path) -> (PathBuf, bool) {
    let base_canon = canonicalize(base_dir);
    if let Ok(cache) = root_cache().lock()
        && let Some(hit) = cache.get(&base_canon)
    {
        return hit.clone();
    }
    let found = find_root_from(&base_canon);
    if let Ok(mut cache) = root_cache().lock() {
        cache.insert(base_canon, found.clone());
    }
    found
}

pub fn check_within_root(base_dir: &Path, resolved: &str) -> Result<(), String> {
    let (root, has_manifest) = project_root_cached(base_dir);
    if !has_manifest {
        return Ok(());
    }
    let res_canon = canonicalize(Path::new(resolved));
    if res_canon.starts_with(&root) {
        Ok(())
    } else {
        Err(format!(
            "E_LAYER: import '{resolved}' escapes project root '{}'",
            root.to_string_lossy()
        ))
    }
}
