use std::path::{Path, PathBuf};

use crate::error::CliError;

const SKIPPED_DIRS: &[&str] = &["target", "node_modules"];

#[derive(Clone, Copy)]
pub enum Depth {
    Direct,
    Recursive,
}

pub fn vn_files(path: &Path, depth: Depth) -> Result<Vec<PathBuf>, CliError> {
    if !path.exists() {
        return Err(CliError::usage(format!(
            "Path '{}' does not exist",
            path.display()
        )));
    }

    let mut files = Vec::new();
    if path.is_file() {
        if is_vn(path) {
            files.push(path.to_path_buf());
        }
        return Ok(files);
    }

    collect(path, depth, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect(dir: &Path, depth: Depth, out: &mut Vec<PathBuf>) -> Result<(), CliError> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        CliError::fatal(format!("Failed to read directory '{}': {e}", dir.display()))
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if matches!(depth, Depth::Recursive) && !skipped(&path) {
                collect(&path, depth, out)?;
            }
        } else if path.is_file() && is_vn(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn skipped(dir: &Path) -> bool {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with('.') || SKIPPED_DIRS.contains(&name)
}

fn is_vn(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == varn_modules::VARN_FILE_EXTENSION)
}
