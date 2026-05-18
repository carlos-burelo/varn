use crate::cli::RemoveArgs;
use crate::error::CliError;
use varn_pm::{
    installer, lockfile,
    manifest::{find_project_manifest, ProjectManifest},
};

pub fn execute(args: RemoveArgs) -> Result<(), CliError> {
    let cwd =
        std::env::current_dir().map_err(|e| CliError::fatal(format!("cannot get cwd: {e}")))?;

    let manifest_path = find_project_manifest(&cwd)
        .ok_or_else(|| CliError::fatal("no varn.json found".to_owned()))?;
    let project_root = manifest_path.parent().unwrap_or(&cwd).to_path_buf();

    let mut manifest = ProjectManifest::load(&manifest_path).map_err(|e| CliError::fatal(e))?;

    if !manifest.dependencies.contains_key(&args.alias) {
        return Err(CliError::fatal(format!(
            "dependency '{}' not found in varn.json",
            args.alias
        )));
    }

    manifest.dependencies.remove(&args.alias);
    manifest
        .save(&manifest_path)
        .map_err(|e| CliError::fatal(e))?;

    let local_pkg = varn_pm::cache::local_package_path(&project_root, &args.alias);
    if local_pkg.exists() {
        std::fs::remove_dir_all(&local_pkg)
            .map_err(|e| CliError::fatal(format!("cannot remove package dir: {e}")))?;
    }

    let lock_path = lockfile::lock_path(&project_root);
    let deps = manifest.parsed_deps().map_err(|e| CliError::fatal(e))?;

    if deps.is_empty() {
        let empty = varn_pm::PmLockfile::empty();
        empty.save(&lock_path).map_err(|e| CliError::fatal(e))?;
    } else {
        let existing = if lock_path.exists() {
            lockfile::PmLockfile::load(&lock_path).ok()
        } else {
            None
        };
        let result = installer::resolve_and_install(&project_root, &deps, existing.as_ref(), false)
            .map_err(|e| CliError::fatal(e))?;
        result
            .lock
            .save(&lock_path)
            .map_err(|e| CliError::fatal(e))?;
    }

    eprintln!("Removed '{}'", args.alias);
    Ok(())
}
