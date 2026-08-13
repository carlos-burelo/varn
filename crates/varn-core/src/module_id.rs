use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModuleScheme {
    Core,

    Runtime,

    Std,

    File,

    Package,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum ModuleId {
    Core(Arc<str>),

    Std(Arc<str>),

    Runtime(Arc<str>),

    Local(Arc<str>),

    Package {
        name: Arc<str>,
        version: Arc<str>,
        entry: Arc<str>,
    },
}

fn normalize_local_path(path: &str) -> Arc<str> {
    if path.starts_with("\\\\?\\") {
        return Arc::from(path[4..].replace('\\', "/").as_str());
    }
    Arc::from(path.replace('\\', "/").as_str())
}

impl ModuleId {
    pub fn core(spec: &str) -> Self {
        Self::Core(Arc::from(spec))
    }

    pub fn stdlib(spec: &str) -> Self {
        if spec.starts_with("core:") {
            Self::Core(Arc::from(spec))
        } else if spec.starts_with("runtime:") {
            Self::Runtime(Arc::from(spec))
        } else {
            Self::Std(Arc::from(spec))
        }
    }

    pub fn runtime(spec: &str) -> Self {
        Self::Runtime(Arc::from(spec))
    }

    pub fn local(path: impl AsRef<Path>) -> Self {
        Self::Local(normalize_local_path(&path.as_ref().to_string_lossy()))
    }

    pub fn local_str(path: &str) -> Self {
        Self::Local(normalize_local_path(path))
    }

    pub fn package(name: &str, version: &str, entry: &str) -> Self {
        Self::Package {
            name: Arc::from(name),
            version: Arc::from(version),
            entry: Arc::from(entry),
        }
    }

    pub fn from_canonical_str(s: &str) -> Self {
        if s.starts_with("core:") {
            Self::Core(Arc::from(s))
        } else if s.starts_with("std:") {
            Self::Std(Arc::from(s))
        } else if s.starts_with("runtime:") {
            Self::Runtime(Arc::from(s))
        } else {
            Self::Local(normalize_local_path(s))
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Core(s) => s.to_string(),
            Self::Std(s) => s.to_string(),
            Self::Runtime(s) => s.to_string(),
            Self::Local(p) => p.to_string(),
            Self::Package {
                name,
                version,
                entry,
            } => format!("{name}@{version}/{entry}"),
        }
    }

    pub fn scheme(&self) -> ModuleScheme {
        match self {
            Self::Core(_) => ModuleScheme::Core,
            Self::Std(_) => ModuleScheme::Std,
            Self::Runtime(_) => ModuleScheme::Runtime,
            Self::Local(_) => ModuleScheme::File,
            Self::Package { .. } => ModuleScheme::Package,
        }
    }

    pub fn is_core(&self) -> bool {
        matches!(self, Self::Core(_))
    }

    pub fn is_std(&self) -> bool {
        matches!(self, Self::Std(_))
    }

    pub fn is_stdlib(&self) -> bool {
        matches!(self, Self::Core(_) | Self::Std(_))
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(s) => write!(f, "{s}"),
            Self::Std(s) => write!(f, "{s}"),
            Self::Runtime(s) => write!(f, "{s}"),
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

    Core(Arc<str>),

    Runtime(Arc<str>),

    Package(Arc<str>),
}

impl ImportSpecifier {
    pub fn parse(raw: &str) -> Self {
        if raw.starts_with('.') {
            Self::Relative(PathBuf::from(raw))
        } else if raw.starts_with("runtime:") {
            Self::Runtime(Arc::from(raw))
        } else if raw.starts_with("core:") {
            Self::Core(Arc::from(raw))
        } else if raw.starts_with("std:") {
            Self::Stdlib(Arc::from(raw))
        } else if raw.starts_with("pkg:") {
            Self::Package(Arc::from(raw))
        } else {
            Self::Relative(PathBuf::from(raw))
        }
    }

}
