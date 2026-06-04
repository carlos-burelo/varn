use crate::{cli::CacheCommands, error::CliError};

pub fn execute(cmd: CacheCommands) -> Result<(), CliError> {
    match cmd {
        CacheCommands::Clean => clean(),
    }
}

fn clean() -> Result<(), CliError> {
    let cwd = std::env::current_dir()
        .map_err(|e| CliError::fatal(format!("error[cache]: cannot get cwd: {e}")))?;

    let project_root = varn_modules::artifact::find_project_root(&cwd);
    let cache_dir = varn_modules::artifact::get_cache_dir(&project_root);

    if !cache_dir.exists() {
        println!("Cache directory does not exist: {}", cache_dir.display());
        return Ok(());
    }

    let mut count = 0usize;
    let mut bytes = 0u64;

    fn clean_dir(dir: &std::path::Path, count: &mut usize, bytes: &mut u64) -> Result<(), CliError> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| CliError::fatal(format!("error[cache]: cannot read dir {}: {e}", dir.display())))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                clean_dir(&path, count, bytes)?;
                let _ = std::fs::remove_dir(&path);
            } else if path.is_file() {
                let ext = path.extension().map(|e| e.to_string_lossy().to_string());
                if ext.as_deref() == Some("vnc") || ext.as_deref() == Some("vnm") || ext.as_deref() == Some("bin") {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        *bytes += meta.len();
                    }
                    std::fs::remove_file(&path).map_err(|e| {
                        CliError::fatal(format!(
                            "error[cache]: cannot remove {}: {e}",
                            path.display()
                        ))
                    })?;
                    *count += 1;
                }
            }
        }
        Ok(())
    }

    clean_dir(&cache_dir, &mut count, &mut bytes)?;

    if count == 0 {
        println!("Cache is already empty.");
    } else {
        println!(
            "Removed {count} cache file(s) ({} freed).",
            format_bytes(bytes)
        );
    }
    Ok(())
}

fn format_bytes(b: u64) -> String {
    if b >= 1024 * 1024 {
        format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}
