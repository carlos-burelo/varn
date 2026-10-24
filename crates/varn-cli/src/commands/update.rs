use crate::error::CliError;
use varn_core::term::terminal;
use varn_pm::{installer, lockfile};

pub fn execute() -> Result<(), CliError> {
    let cwd =
        std::env::current_dir().map_err(|e| CliError::fatal(format!("cannot get cwd: {e}")))?;

    let manifest_path = varn_project::project_manifest(&cwd)
        .ok_or_else(|| CliError::fatal("no varn.toml found".to_owned()))?;
    let project_root = manifest_path.parent().unwrap_or(&cwd).to_path_buf();

    let manifest = varn_project::Manifest::load(&manifest_path)?;
    let deps = varn_pm::parse_dependencies(&manifest.dependencies).map_err(CliError::fatal)?;

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
