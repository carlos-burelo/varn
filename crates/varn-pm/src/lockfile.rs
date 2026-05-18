use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LOCK_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockPackage {
    pub name: String,
    pub origin: String,
    pub version: String,
    pub commit: String,
    pub integrity: String,
    pub resolved: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PmLockfile {
    pub version: u32,
    pub packages: Vec<LockPackage>,
}

impl PmLockfile {
    pub fn empty() -> Self {
        Self {
            version: LOCK_VERSION,
            packages: vec![],
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let raw =
            std::fs::read_to_string(path).map_err(|e| format!("cannot read lockfile: {e}"))?;
        let lock: Self =
            serde_json::from_str(&raw).map_err(|e| format!("invalid lockfile: {e}"))?;
        Ok(lock)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize lockfile: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("cannot write lockfile: {e}"))
    }

    pub fn by_name(&self) -> HashMap<&str, &LockPackage> {
        self.packages.iter().map(|p| (p.name.as_str(), p)).collect()
    }

    pub fn mismatch(&self, other: &Self) -> bool {
        if self.packages.len() != other.packages.len() {
            return true;
        }
        let mine = self.by_name();
        let theirs = other.by_name();
        mine.iter().any(|(name, a)| {
            theirs.get(name).map_or(true, |b| {
                a.origin != b.origin
                    || a.version != b.version
                    || a.commit != b.commit
                    || a.integrity != b.integrity
            })
        })
    }
}

pub fn lock_path(project_root: &Path) -> PathBuf {
    project_root
        .join(varn_modules::ENV_DIR_NAME)
        .join("varn.lock")
}
