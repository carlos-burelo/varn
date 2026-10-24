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

pub fn nearest(start: &Path) -> Located {
    let start = absolute(start);
    for dir in start.ancestors() {
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

pub fn project_manifest(start: &Path) -> Option<PathBuf> {
    let start = absolute(start);
    start
        .ancestors()
        .filter(|dir| !inside_installed_packages(dir))
        .map(|dir| dir.join(MANIFEST_FILE))
        .find(|candidate| candidate.exists())
}
