use std::collections::HashSet;
use std::path::Path;

use crate::{
    cache, fetcher,
    lockfile::{LockPackage, PmLockfile},
    manifest::{DepOrigin, ProjectManifest},
    resolver,
};

pub struct InstallResult {
    pub lock: PmLockfile,
}

pub fn resolve_and_install(
    project_root: &Path,
    deps: &std::collections::HashMap<String, DepOrigin>,
    existing_lock: Option<&PmLockfile>,
    force_update: bool,
) -> Result<InstallResult, String> {
    let mut packages: Vec<LockPackage> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();

    resolve_recursive(
        project_root,
        deps,
        existing_lock,
        force_update,
        &mut packages,
        &mut visited,
    )?;

    packages.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(InstallResult {
        lock: PmLockfile {
            version: crate::lockfile::LOCK_VERSION,
            packages,
        },
    })
}

fn resolve_recursive(
    project_root: &Path,
    deps: &std::collections::HashMap<String, DepOrigin>,
    existing_lock: Option<&PmLockfile>,
    force_update: bool,
    packages: &mut Vec<LockPackage>,
    visited: &mut HashSet<String>,
) -> Result<(), String> {
    for (alias, origin) in deps {
        if !visited.insert(alias.clone()) {
            continue; // Avoid infinite loops in cyclic dependencies
        }

        let resolved = if !force_update {
            if let Some(lock) = existing_lock {
                if let Some(entry) = lock.by_name().get(alias.as_str()) {
                    if let Ok(dep_origin) = DepOrigin::parse(&entry.origin) {
                        if dep_origin == *origin
                            && cache::is_cached(&dep_origin, &entry.version, &entry.integrity)
                        {
                            packages.push((*entry).clone());
                            install_from_cache(project_root, alias, &dep_origin, &entry.version)?;
                            continue;
                        }
                    }
                }
            }
            resolver::resolve_version(origin)?
        } else {
            resolver::resolve_version(origin)?
        };

        let integrity = match origin {
            DepOrigin::LocalPath { path: src_path } => {
                let dst = cache::local_package_path(project_root, alias);
                if dst.exists() {
                    let _ = std::fs::remove_dir_all(&dst);
                }
                copy_dir_all(src_path, &dst)?;
                "sha256:local".to_owned()
            }
            DepOrigin::Remote { .. } => {
                let dest = cache::cached_package_path(origin, &resolved.version);
                let integrity = if cache::is_cached(origin, &resolved.version, "") {
                    let integrity_path = dest.join(".integrity");
                    std::fs::read_to_string(&integrity_path)
                        .map(|s| s.trim().to_owned())
                        .unwrap_or_else(|_| "sha256:unknown".to_owned())
                } else {
                    let integrity = fetcher::fetch_and_extract(origin, &resolved.version, &dest)?;
                    fetcher::write_integrity_file(&dest, &integrity)?;
                    integrity
                };

                install_from_cache(project_root, alias, origin, &resolved.version)?;
                integrity
            }
        };

        packages.push(LockPackage {
            name: alias.clone(),
            origin: origin.to_origin_string(),
            version: resolved.version.clone(),
            commit: resolved.commit.clone(),
            integrity,
            resolved: origin.tarball_url(&resolved.version),
        });

        // Inspect child varn.json for transitive dependencies
        let installed_pkg_dir = cache::local_package_path(project_root, alias);
        let child_manifest_path = installed_pkg_dir.join(varn_modules::PACKAGE_MANIFEST_FILE);
        if child_manifest_path.exists() {
            if let Ok(child_manifest) = ProjectManifest::load(&child_manifest_path) {
                if let Ok(child_deps) = child_manifest.parsed_deps() {
                    resolve_recursive(
                        project_root,
                        &child_deps,
                        existing_lock,
                        force_update,
                        packages,
                        visited,
                    )?;
                }
            }
        }
    }

    Ok(())
}

pub fn install_locked(project_root: &Path, lock: &PmLockfile) -> Result<(), String> {
    for pkg in &lock.packages {
        let origin = DepOrigin::parse(&pkg.origin)?;

        match &origin {
            DepOrigin::LocalPath { path: src_path } => {
                let dst = cache::local_package_path(project_root, &pkg.name);
                if dst.exists() {
                    let _ = std::fs::remove_dir_all(&dst);
                }
                copy_dir_all(src_path, &dst)?;
            }
            DepOrigin::Remote { .. } => {
                if !cache::is_cached(&origin, &pkg.version, &pkg.integrity) {
                    varn_utilities::terminal::info(format!(
                        "downloading {}@{} ...",
                        pkg.name, pkg.version
                    ));
                    let dest = cache::cached_package_path(&origin, &pkg.version);
                    let actual = fetcher::fetch_and_extract(&origin, &pkg.version, &dest)?;
                    if actual != pkg.integrity && pkg.integrity != "sha256:unknown" {
                        return Err(format!(
                            "integrity mismatch for '{}': expected {}, got {}",
                            pkg.name, pkg.integrity, actual
                        ));
                    }
                    fetcher::write_integrity_file(&dest, &actual)?;
                }

                install_from_cache(project_root, &pkg.name, &origin, &pkg.version)?;
            }
        }
    }
    Ok(())
}

fn install_from_cache(
    project_root: &Path,
    alias: &str,
    origin: &DepOrigin,
    version: &str,
) -> Result<(), String> {
    let src = cache::cached_package_path(origin, version);
    let dst = cache::local_package_path(project_root, alias);

    if dst.exists() {
        let src_integrity = src.join(".integrity");
        let dst_integrity = dst.join(".integrity");
        if let (Ok(a), Ok(b)) = (
            std::fs::read_to_string(&src_integrity),
            std::fs::read_to_string(&dst_integrity),
        ) {
            if a.trim() == b.trim() {
                return Ok(());
            }
        }
        std::fs::remove_dir_all(&dst)
            .map_err(|e| format!("cannot remove {}: {e}", dst.display()))?;
    }

    copy_dir_all(&src, &dst)
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {e}", dst.display()))?;

    for entry in
        std::fs::read_dir(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?
    {
        let entry = entry.map_err(|e| format!("dir entry error: {e}"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| {
                format!(
                    "cannot copy {} → {}: {e}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}
