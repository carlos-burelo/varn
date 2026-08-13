use crate::error::CliError;
use varn_pm::{
    installer, lockfile,
    manifest::{find_project_manifest, ProjectManifest},
};
use varn_term::terminal;

pub fn execute() -> Result<(), CliError> {
    let cwd =
        std::env::current_dir().map_err(|e| CliError::fatal(format!("cannot get cwd: {e}")))?;

    let manifest_path = find_project_manifest(&cwd)
        .ok_or_else(|| CliError::fatal("no varn.json found — run `vn init` first".to_owned()))?;
    let project_root = manifest_path.parent().unwrap_or(&cwd).to_path_buf();

    let lock_path = lockfile::lock_path(&project_root);

    if lock_path.exists() {
        let lock = lockfile::PmLockfile::load(&lock_path).map_err(|e| CliError::fatal(e))?;
        if lock.packages.is_empty() {
            terminal::log("Nothing to install.");
            return Ok(());
        }
        terminal::log(format!(
            "Installing {} package(s) from lockfile...",
            lock.packages.len()
        ));
        installer::install_locked(&project_root, &lock).map_err(|e| CliError::fatal(e))?;
    } else {
        let manifest = ProjectManifest::load(&manifest_path).map_err(|e| CliError::fatal(e))?;
        let deps = manifest.parsed_deps().map_err(|e| CliError::fatal(e))?;
        if deps.is_empty() {
            terminal::log("No dependencies declared in varn.json.");
            return Ok(());
        }
        terminal::log(format!("Resolving {} dependency(ies)...", deps.len()));
        let result = installer::resolve_and_install(&project_root, &deps, None, false)
            .map_err(|e| CliError::fatal(e))?;
        result
            .lock
            .save(&lock_path)
            .map_err(|e| CliError::fatal(e))?;
        terminal::log("Lockfile written.");
    }

    terminal::log("Done.");
    Ok(())
}
