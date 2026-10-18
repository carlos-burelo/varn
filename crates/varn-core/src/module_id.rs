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

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModuleKind {
    Core,
    Stdlib,
    Runtime,
}

impl ModuleKind {
    pub const ALL: [Self; 3] = [Self::Core, Self::Stdlib, Self::Runtime];

    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Core => "core:",
            Self::Stdlib => "std:",
            Self::Runtime => "runtime:",
        }
    }

    pub fn of(spec: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| spec.starts_with(k.prefix()))
    }

    pub fn owns(self, spec: &str) -> bool {
        spec.starts_with(self.prefix())
    }
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
    if let Some(stripped) = path.strip_prefix(r"\\?\") {
        return Arc::from(stripped.replace('\\', "/").as_str());
    }
    Arc::from(path.replace('\\', "/").as_str())
}

impl ModuleId {
    pub fn core(spec: &str) -> Self {
        Self::Core(Arc::from(spec))
    }

    pub fn stdlib(spec: &str) -> Self {
        match ModuleKind::of(spec) {
            Some(ModuleKind::Core) => Self::Core(Arc::from(spec)),
            Some(ModuleKind::Runtime) => Self::Runtime(Arc::from(spec)),
            Some(ModuleKind::Stdlib) | None => Self::Std(Arc::from(spec)),
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
        match ModuleKind::of(s) {
            Some(ModuleKind::Core) => Self::Core(Arc::from(s)),
            Some(ModuleKind::Stdlib) => Self::Std(Arc::from(s)),
            Some(ModuleKind::Runtime) => Self::Runtime(Arc::from(s)),
            None => Self::Local(normalize_local_path(s)),
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
        if is_file_specifier(raw) {
            return Self::Relative(PathBuf::from(raw));
        }
        match ModuleKind::of(raw) {
            Some(ModuleKind::Runtime) => Self::Runtime(Arc::from(raw)),
            Some(ModuleKind::Core) => Self::Core(Arc::from(raw)),
            Some(ModuleKind::Stdlib) => Self::Stdlib(Arc::from(raw)),
            None => Self::Package(Arc::from(raw)),
        }
    }
}

fn is_file_specifier(raw: &str) -> bool {
    if raw.starts_with('.') || raw.starts_with('/') || raw.starts_with('\\') {
        return true;
    }
    let b = raw.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return true;
    }
    false
}
