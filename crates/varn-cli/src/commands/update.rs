use crate::error::CliError;
use varn_pm::{
    installer, lockfile,
    manifest::{find_project_manifest, ProjectManifest},
};
use varn_utilities::terminal;

pub fn execute() -> Result<(), CliError> {
    let cwd =
        std::env::current_dir().map_err(|e| CliError::fatal(format!("cannot get cwd: {e}")))?;

    let manifest_path = find_project_manifest(&cwd)
        .ok_or_else(|| CliError::fatal("no varn.json found".to_owned()))?;
    let project_root = manifest_path.parent().unwrap_or(&cwd).to_path_buf();

    let manifest = ProjectManifest::load(&manifest_path).map_err(|e| CliError::fatal(e))?;
    let deps = manifest.parsed_deps().map_err(|e| CliError::fatal(e))?;

    if deps.is_empty() {
        terminal::log("No dependencies to update.");
        return Ok(());
    }

    terminal::log(format!("Updating {} dependency(ies)...", deps.len()));

    let result = installer::resolve_and_install(&project_root, &deps, None, true)
        .map_err(|e| CliError::fatal(e))?;

    result
        .lock
        .save(&lockfile::lock_path(&project_root))
        .map_err(|e| CliError::fatal(e))?;

    for pkg in &result.lock.packages {
        terminal::log(format!("  {} → v{}", pkg.name, pkg.version));
    }
    terminal::log("Lockfile updated.");
    Ok(())
}
