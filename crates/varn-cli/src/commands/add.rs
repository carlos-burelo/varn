use crate::cli::AddArgs;
use crate::error::CliError;
use varn_core::term::terminal;
use varn_pm::{installer, lockfile, DepOrigin};
use varn_project::Project;

pub fn execute(project: &Project, args: AddArgs) -> Result<(), CliError> {
    let manifest_path = super::pkg::manifest_path(project)?;
    let project_root = project.root.as_path();

    DepOrigin::parse(&args.origin).map_err(CliError::fatal)?;

    let mut manifest = project.manifest.clone();

    manifest
        .dependencies
        .insert(args.alias.clone(), args.origin.clone());
    manifest.save(manifest_path)?;

    let deps = varn_pm::parse_dependencies(&manifest.dependencies).map_err(CliError::fatal)?;

    let lock_path = lockfile::lock_path(&project_root);
    let existing_lock = if lock_path.exists() {
        lockfile::PmLockfile::load(&lock_path).ok()
    } else {
        None
    };

    terminal::log(format!("Resolving {}...", args.alias));

    let result =
        installer::resolve_and_install(&project_root, &deps, existing_lock.as_ref(), false)
            .map_err(CliError::fatal)?;

    result.lock.save(&lock_path).map_err(CliError::fatal)?;

    terminal::log(format!("Added {} → {}", args.alias, args.origin));
    Ok(())
}
