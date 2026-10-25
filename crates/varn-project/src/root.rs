use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::names::{ENV_DIR_NAME, MANIFEST_FILE, MODULES_DIR_NAME};

pub struct Located {
    pub dir: PathBuf,
    pub manifest: Option<PathBuf>,
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Nearest,
    Owner,
}

pub fn locate(start: &Path, scope: Scope) -> Located {
    let start = absolute(start);
    for dir in start.ancestors() {
        if scope == Scope::Owner && inside_installed_packages(dir) {
            continue;
        }
        let candidate = dir.join(MANIFEST_FILE);
        if candidate.exists() {
            return Located {
                dir: dir.to_path_buf(),
                manifest: Some(candidate),
            };
        }
    }
    Located {
        dir: start,
        manifest: None,
    }
}

pub fn nearest(start: &Path) -> Located {
    locate(start, Scope::Nearest)
}

fn inside_installed_packages(dir: &Path) -> bool {
    let mut previous: Option<&OsStr> = None;
    for component in dir.components() {
        let current = component.as_os_str();
        if previous == Some(OsStr::new(ENV_DIR_NAME)) && current == OsStr::new(MODULES_DIR_NAME) {
            return true;
        }
        previous = Some(current);
    }
    false
}
