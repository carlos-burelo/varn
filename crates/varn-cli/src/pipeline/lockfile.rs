use crate::error::CliError;
use std::path::Path;
use varn_types::{ModuleGraphArtifact, PackageNode};

#[derive(serde::Serialize, serde::Deserialize)]
struct Lockfile {
    version: u32,
    entry: String,
    packages: Vec<LockPackage>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct LockPackage {
    specifier: String,
    package: String,
    version: String,
    subpath: String,
    resolved_path: String,
    integrity: String,
}

const LOCK_VERSION: u32 = 1;

pub fn sync_lockfile(entry_file: &str, graph: &ModuleGraphArtifact) -> Result<(), CliError> {
    let entry_path = Path::new(entry_file);
    let project_root = entry_path.parent().unwrap_or_else(|| Path::new("."));
    let env_dir = project_root.join(varn_modules::ENV_DIR_NAME);
    let lock_path = env_dir.join("varn.lock");

    if !env_dir.exists() {
        std::fs::create_dir_all(&env_dir)
            .map_err(|e| CliError::fatal(format!("cannot create .vn directory: {e}")))?;
    }

    let relativize = |p: &str| -> String {
        let pb = Path::new(p);
        if pb.is_absolute() {
            if let Ok(rel) = pb.strip_prefix(project_root) {
                return rel.to_string_lossy().replace('\\', "/");
            }
        }
        p.replace('\\', "/")
    };

    let next = Lockfile {
        version: LOCK_VERSION,
        entry: relativize(&graph.entry_path),
        packages: sorted_packages(&graph.package_nodes, &relativize),
    };

    if lock_path.exists() {
        let raw = std::fs::read_to_string(&lock_path).map_err(|e| {
            CliError::fatal(format!(
                "cannot read lockfile '{}': {e}",
                lock_path.display()
            ))
        })?;
        let prev: Lockfile = serde_json::from_str(&raw).map_err(|e| {
            CliError::fatal(format!("invalid lockfile '{}': {e}", lock_path.display()))
        })?;
        if lock_mismatch(&prev, &next) {
            if std::env::var("VARN_LOCK_UPDATE").ok().as_deref() == Some("1") {
                return write_lockfile(&lock_path, &next);
            }
            return Err(CliError::fatal(format!(
                "lockfile mismatch at '{}'. Re-run with VARN_LOCK_UPDATE=1 to refresh.",
                lock_path.display(),
            )));
        }
        return Ok(());
    }

    write_lockfile(&lock_path, &next)
}

fn sorted_packages<F>(nodes: &[PackageNode], relativize: &F) -> Vec<LockPackage>
where
    F: Fn(&str) -> String,
{
    let mut out: Vec<LockPackage> = nodes
        .iter()
        .map(|n| LockPackage {
            specifier: n.specifier.clone(),
            package: n.package.clone(),
            version: n.version.clone(),
            subpath: n.subpath.clone(),
            resolved_path: relativize(&n.resolved_path),
            integrity: n.integrity.clone(),
        })
        .collect();
    out.sort_by(|a, b| a.specifier.cmp(&b.specifier));
    out
}

fn lock_mismatch(prev: &Lockfile, next: &Lockfile) -> bool {
    if prev.version != next.version {
        return true;
    }
    prev.packages.len() != next.packages.len()
        || prev
            .packages
            .iter()
            .zip(next.packages.iter())
            .any(|(a, b)| {
                a.specifier != b.specifier
                    || a.package != b.package
                    || a.version != b.version
                    || a.subpath != b.subpath
                    || a.resolved_path != b.resolved_path
                    || a.integrity != b.integrity
            })
}

fn write_lockfile(lock_path: &Path, lock: &Lockfile) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(lock)
        .map_err(|e| CliError::fatal(format!("cannot serialize lockfile: {e}")))?;
    std::fs::write(lock_path, json).map_err(|e| {
        CliError::fatal(format!(
            "cannot write lockfile '{}': {e}",
            lock_path.display()
        ))
    })
}
