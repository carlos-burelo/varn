use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum ModuleId {
    Stdlib(Arc<str>),

    Local(Arc<str>),

    Package {
        name: Arc<str>,
        version: Arc<str>,
        entry: Arc<str>,
    },
}

impl ModuleId {
    pub fn stdlib(spec: &str) -> Self {
        Self::Stdlib(Arc::from(spec))
    }

    pub fn local(path: impl AsRef<Path>) -> Self {
        Self::Local(Arc::from(path.as_ref().to_string_lossy().as_ref()))
    }

    pub fn local_str(path: &str) -> Self {
        Self::Local(Arc::from(path))
    }

    pub fn package(name: &str, version: &str, entry: &str) -> Self {
        Self::Package {
            name: Arc::from(name),
            version: Arc::from(version),
            entry: Arc::from(entry),
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Stdlib(s) => s.to_string(),
            Self::Local(p) => p.to_string(),
            Self::Package {
                name,
                version,
                entry,
            } => format!("{name}@{version}/{entry}"),
        }
    }

    pub fn is_stdlib(&self) -> bool {
        matches!(self, Self::Stdlib(_))
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdlib(s) => write!(f, "{s}"),
            Self::Local(p) => write!(f, "{p}"),
            Self::Package {
                name,
                version,
                entry,
            } => write!(f, "{name}@{version}/{entry}"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ImportSpecifier {
    Relative(PathBuf),

    Stdlib(Arc<str>),

    Package(Arc<str>),
}

impl ImportSpecifier {
    pub fn parse(raw: &str) -> Self {
        if raw.starts_with('.') {
            Self::Relative(PathBuf::from(raw))
        } else if raw.starts_with("std:") || raw.starts_with("builtin:") {
            Self::Stdlib(Arc::from(raw))
        } else if raw.starts_with("pkg:") {
            Self::Package(Arc::from(raw))
        } else {
            let is_absolute = raw.starts_with('/') || {
                let bytes = raw.as_bytes();
                bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'/' || bytes[2] == b'\\')
            };
            if is_absolute {
                Self::Relative(PathBuf::from(raw))
            } else {
                Self::Relative(PathBuf::from(raw))
            }
        }
    }
}
