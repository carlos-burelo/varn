use std::path::PathBuf;

use crate::manifest::DepOrigin;

pub fn global_cache_root() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_owned());
    PathBuf::from(home)
        .join(varn_modules::ENV_DIR_NAME)
        .join("cache")
}

pub fn cached_package_path(origin: &DepOrigin, version: &str) -> PathBuf {
    global_cache_root()
        .join(origin.host.replace('.', "_"))
        .join(&origin.user)
        .join(&origin.repo)
        .join(version)
}

pub fn local_package_path(project_root: &std::path::Path, alias: &str) -> PathBuf {
    project_root
        .join(varn_modules::ENV_DIR_NAME)
        .join(varn_modules::MODULES_DIR_NAME)
        .join(alias)
}

pub fn is_cached(origin: &DepOrigin, version: &str, expected_integrity: &str) -> bool {
    let path = cached_package_path(origin, version);
    crate::fetcher::verify_cached(&path, expected_integrity)
}
