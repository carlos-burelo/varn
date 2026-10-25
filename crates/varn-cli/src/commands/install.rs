use crate::error::CliError;
use varn_core::term::terminal;
use varn_pm::{installer, lockfile};
use varn_project::Project;

pub fn execute(project: &Project) -> Result<(), CliError> {
    super::pkg::manifest_path(project)?;
    let project_root = project.root.as_path();

    let lock_path = lockfile::lock_path(&project_root);

    if lock_path.exists() {
        let lock = lockfile::PmLockfile::load(&lock_path).map_err(CliError::fatal)?;
        if lock.packages.is_empty() {
            terminal::log("Nothing to install.");
            return Ok(());
        }
        terminal::log(format!(
            "Installing {} package(s) from lockfile...",
            lock.packages.len()
        ));
        installer::install_locked(&project_root, &lock).map_err(CliError::fatal)?;
    } else {
        let deps =
            varn_pm::parse_dependencies(&project.manifest.dependencies).map_err(CliError::fatal)?;
        if deps.is_empty() {
            terminal::log("No dependencies declared in varn.toml.");
            return Ok(());
        }
        terminal::log(format!("Resolving {} dependency(ies)...", deps.len()));
        let result = installer::resolve_and_install(&project_root, &deps, None, false)
            .map_err(CliError::fatal)?;
        result.lock.save(&lock_path).map_err(CliError::fatal)?;
        terminal::log("Lockfile written.");
    }

    terminal::log("Done.");
    Ok(())
}
