use std::path::{Path, PathBuf};

use crate::names::ENV_DIR_NAME;
use crate::root::nearest;

pub fn apply_from_cwd() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = nearest(&cwd).dir;

    let mut dirs: Vec<&Path> = vec![cwd.as_path()];
    if root != cwd {
        dirs.push(root.as_path());
    }

    for dir in &dirs {
        apply_file(&dir.join(".env.local"));
    }
    for dir in &dirs {
        apply_file(&dir.join(".env"));
    }
    for dir in &dirs {
        apply_file(&dir.join(ENV_DIR_NAME).join(".env"));
    }
}

fn apply_file(path: &Path) {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || std::env::var_os(key).is_some() {
            continue;
        }
        unsafe {
            std::env::set_var(key, unquote(value.trim()));
        }
    }
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return &value[1..value.len() - 1];
        }
    }
    value
}
