use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::names::MANIFEST_FILE;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub std: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exports: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
    #[serde(
        default,
        alias = "dev-dependencies",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub dev_dependencies: BTreeMap<String, String>,
    #[serde(
        default,
        alias = "peer-dependencies",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub peer_dependencies: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tasks: BTreeMap<String, Vec<String>>,
}

#[derive(Debug)]
pub enum ManifestError {
    Read { path: PathBuf, message: String },
    Invalid { path: PathBuf, message: String },
    Serialize(String),
    Write { path: PathBuf, message: String },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Read { path, message } => {
                write!(f, "cannot read {}: {message}", path.display())
            }
            ManifestError::Invalid { path, message } => {
                write!(f, "invalid {}: {message}", path.display())
            }
            ManifestError::Serialize(message) => {
                write!(f, "cannot serialize manifest: {message}")
            }
            ManifestError::Write { path, message } => {
                write!(f, "cannot write {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ManifestError {}

impl Manifest {
    pub fn load(path: &Path) -> Result<Manifest, ManifestError> {
        let raw = std::fs::read_to_string(path).map_err(|e| ManifestError::Read {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        toml::from_str(&raw).map_err(|e| ManifestError::Invalid {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    pub fn load_in(dir: &Path) -> Result<Option<Manifest>, ManifestError> {
        let path = dir.join(MANIFEST_FILE);
        if !path.exists() {
            return Ok(None);
        }
        Manifest::load(&path).map(Some)
    }

    pub fn save(&self, path: &Path) -> Result<(), ManifestError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| ManifestError::Serialize(e.to_string()))?;
        std::fs::write(path, content).map_err(|e| ManifestError::Write {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }
}
