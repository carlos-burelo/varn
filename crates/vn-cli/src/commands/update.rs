use crate::error::CliError;
use vn_core::term::terminal;
use vn_pm::{installer, lockfile};
use vn_project::Project;

pub fn execute(project: &Project) -> Result<(), CliError> {
    super::pkg::manifest_path(project)?;
    let project_root = project.root.as_path();

    let deps =
        vn_pm::parse_dependencies(&project.manifest.dependencies).map_err(CliError::fatal)?;

    if deps.is_empty() {
        terminal::log("No dependencies to update.");
        return Ok(());
    }

    terminal::log(format!("Updating {} dependency(ies)...", deps.len()));

    let result = installer::resolve_and_install(&project_root, &deps, None, true)
        .map_err(CliError::fatal)?;

    result
        .lock
        .save(&lockfile::lock_path(&project_root))
        .map_err(CliError::fatal)?;

    for pkg in &result.lock.packages {
        terminal::log(format!("  {} → v{}", pkg.name, pkg.version));
    }
    terminal::log("Lockfile updated.");
    Ok(())
}
