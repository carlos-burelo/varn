use crate::cli::RemoveArgs;
use crate::error::CliError;
use varn_core::term::terminal;
use varn_pm::{installer, lockfile};
use varn_project::Project;

pub fn execute(project: &Project, args: RemoveArgs) -> Result<(), CliError> {
    let manifest_path = super::pkg::manifest_path(project)?;
    let project_root = project.root.as_path();

    let mut manifest = project.manifest.clone();

    if !manifest.dependencies.contains_key(&args.alias) {
        return Err(CliError::fatal(format!(
            "dependency '{}' not found in varn.toml",
            args.alias
        )));
    }

    manifest.dependencies.remove(&args.alias);
    manifest.save(manifest_path)?;

    let local_pkg = varn_pm::cache::local_package_path(&project_root, &args.alias);
    if local_pkg.exists() {
        std::fs::remove_dir_all(&local_pkg)
            .map_err(|e| CliError::fatal(format!("cannot remove package dir: {e}")))?;
    }

    let lock_path = lockfile::lock_path(&project_root);
    let deps = varn_pm::parse_dependencies(&manifest.dependencies).map_err(CliError::fatal)?;

    if deps.is_empty() {
        if lock_path.exists() {
            std::fs::remove_file(&lock_path)
                .map_err(|e| CliError::fatal(format!("cannot remove lockfile: {e}")))?;
        }
    } else {
        let existing = if lock_path.exists() {
            lockfile::PmLockfile::load(&lock_path).ok()
        } else {
            None
        };
        let result = installer::resolve_and_install(&project_root, &deps, existing.as_ref(), false)
            .map_err(CliError::fatal)?;
        result.lock.save(&lock_path).map_err(CliError::fatal)?;
    }

    terminal::log(format!("Removed '{}'", args.alias));
    Ok(())
}
