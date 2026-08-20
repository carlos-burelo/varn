use crate::cli::AddArgs;
use crate::error::CliError;
use varn_pm::{
    installer, lockfile,
    manifest::{find_project_manifest, DepOrigin, ProjectManifest},
};
use varn_term::terminal;

pub fn execute(args: AddArgs) -> Result<(), CliError> {
    let cwd =
        std::env::current_dir().map_err(|e| CliError::fatal(format!("cannot get cwd: {e}")))?;

    let manifest_path = find_project_manifest(&cwd)
        .ok_or_else(|| CliError::fatal("no varn.toml found — run `vn init` first".to_owned()))?;
    let project_root = manifest_path.parent().unwrap_or(&cwd).to_path_buf();

    DepOrigin::parse(&args.origin).map_err(|e| CliError::fatal(e))?;

    let mut manifest = ProjectManifest::load(&manifest_path).map_err(|e| CliError::fatal(e))?;

    manifest
        .dependencies
        .insert(args.alias.clone(), args.origin.clone());
    manifest
        .save(&manifest_path)
        .map_err(|e| CliError::fatal(e))?;

    let deps = manifest.parsed_deps().map_err(|e| CliError::fatal(e))?;

    let lock_path = lockfile::lock_path(&project_root);
    let existing_lock = if lock_path.exists() {
        lockfile::PmLockfile::load(&lock_path).ok()
    } else {
        None
    };

    terminal::log(format!("Resolving {}...", args.alias));

    let result =
        installer::resolve_and_install(&project_root, &deps, existing_lock.as_ref(), false)
            .map_err(|e| CliError::fatal(e))?;

    result
        .lock
        .save(&lock_path)
        .map_err(|e| CliError::fatal(e))?;

    terminal::log(format!("Added {} → {}", args.alias, args.origin));
    Ok(())
}
