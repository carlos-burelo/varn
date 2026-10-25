use crate::{cli::PkgCommands, error::CliError};
use std::path::Path;
use varn_core::term::terminal;
use varn_project::Project;

pub fn manifest_path(project: &Project) -> Result<&Path, CliError> {
    project
        .manifest_path
        .as_deref()
        .ok_or_else(|| CliError::fatal("no varn.toml found — run `vn init` first"))
}

pub fn execute(project: &Project, cmd: PkgCommands) -> Result<(), CliError> {
    match cmd {
        PkgCommands::Add(args) => super::add::execute(project, args),
        PkgCommands::Remove(args) => super::remove::execute(project, args),
        PkgCommands::Install => super::install::execute(project),
        PkgCommands::Update => super::update::execute(project),
        PkgCommands::Tree => execute_tree(project),
        PkgCommands::Doctor => execute_doctor(project),
        PkgCommands::Clean => execute_clean(),
    }
}

fn execute_tree(project: &Project) -> Result<(), CliError> {
    manifest_path(project)?;
    let project_root = project.root.as_path();
    let manifest = &project.manifest;
    let project_name = manifest.name.as_deref().unwrap_or("project");
    let version = manifest.version.as_deref().unwrap_or("0.0.0");

    terminal::log(format!(
        "{project_name}@{version} ({})",
        project_root.display()
    ));

    let lock_path = varn_pm::lockfile::lock_path(project_root);
    if lock_path.exists()
        && let Ok(lock) = varn_pm::PmLockfile::load(&lock_path)
    {
        let total = lock.packages.len();
        for (idx, pkg) in lock.packages.iter().enumerate() {
            let is_last = idx + 1 == total;
            let branch = if is_last { "└── " } else { "├── " };
            terminal::log(format!(
                "{branch}{}@{} ({})",
                pkg.name, pkg.version, pkg.origin
            ));
        }
        return Ok(());
    }

    let deps = varn_pm::parse_dependencies(&manifest.dependencies).map_err(CliError::fatal)?;
    let total = deps.len();
    for (idx, (alias, origin)) in deps.iter().enumerate() {
        let is_last = idx + 1 == total;
        let branch = if is_last { "└── " } else { "├── " };
        terminal::log(format!("{branch}{alias} → {}", origin.to_origin_string()));
    }

    Ok(())
}

fn execute_doctor(project: &Project) -> Result<(), CliError> {
    manifest_path(project)?;
    let project_root = project.root.as_path();

    terminal::log("Auditing project package integrity...");

    let lock_path = varn_pm::lockfile::lock_path(project_root);
    if !lock_path.exists() {
        terminal::info("No varn.lock found. Run `vn pkg install` to generate lockfile.");
        return Ok(());
    }

    let lock = varn_pm::PmLockfile::load(&lock_path).map_err(CliError::fatal)?;
    let issues = lock.verify_project_integrity(project_root);

    if issues.is_empty() {
        terminal::log("✔ All packages and lockfile checksums are intact!");
    } else {
        terminal::log(format!("⚠ Found {} issue(s):", issues.len()));
        for issue in issues {
            terminal::info(format!("  - {issue}"));
        }
    }

    Ok(())
}

fn execute_clean() -> Result<(), CliError> {
    terminal::log("Cleaning global package cache (~/.vn/cache)...");
    match varn_pm::cache::clean_cache() {
        Ok(count) => {
            terminal::log(format!("✔ Cleaned {count} cached package(s)."));
            Ok(())
        }
        Err(e) => Err(CliError::fatal(e)),
    }
}
