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
    match origin {
        DepOrigin::Remote {
            host, user, repo, ..
        } => global_cache_root()
            .join(host.replace('.', "_"))
            .join(user)
            .join(repo)
            .join(version),
        DepOrigin::LocalPath { path } => path.clone(),
    }
}

pub fn local_package_path(project_root: &std::path::Path, alias: &str) -> PathBuf {
    project_root
        .join(varn_modules::ENV_DIR_NAME)
        .join(varn_modules::MODULES_DIR_NAME)
        .join(alias)
}

pub fn is_cached(origin: &DepOrigin, version: &str, expected_integrity: &str) -> bool {
    match origin {
        DepOrigin::LocalPath { path } => path.exists(),
        DepOrigin::Remote { .. } => {
            let path = cached_package_path(origin, version);
            crate::fetcher::verify_cached(&path, expected_integrity)
        }
    }
}

pub fn clean_cache() -> Result<usize, String> {
    let cache_dir = global_cache_root();
    if !cache_dir.exists() {
        return Ok(0);
    }
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let _ = std::fs::remove_dir_all(entry.path());
                count += 1;
            }
        }
    }
    Ok(count)
}
