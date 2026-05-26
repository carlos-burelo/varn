use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Canonical scheme for a module — encodes which layer owns it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModuleScheme {
    /// Intrinsic domains: primitive method tables, compiler-known symbols.
    /// Not user-importable; injected by the runtime.
    Core,
    /// ABI boundary between stdlib and OS/VM. Unstable, runtime-versioned.
    Runtime,
    /// Public standard library. Stable API, uses runtime: internally.
    Std,
    /// Filesystem path (user files, relative imports).
    File,
    /// User-installed packages resolved via manifest + lockfile.
    Package,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum ModuleId {
    /// core:* — compiler-internal intrinsic domain. Not user-importable.
    Core(Arc<str>),

    /// std:* — public standard library with stable API.
    Std(Arc<str>),

    /// runtime:* — ABI boundary, resolved to native ops.
    Runtime(Arc<str>),

    /// Canonical filesystem path (normalized: forward slashes, lowercase drive letter).
    Local(Arc<str>),

    /// User-installed package resolved via manifest + lockfile.
    Package {
        name: Arc<str>,
        version: Arc<str>,
        entry: Arc<str>,
    },
}

fn normalize_local_path(path: &str) -> Arc<str> {
    // Match varn_modules::normalize_path_string: strip \\?\ prefix, forward slashes.
    // Drive-letter case normalization deferred until precompiled map is also ModuleId-keyed.
    if path.starts_with("\\\\?\\") {
        return Arc::from(path[4..].replace('\\', "/").as_str());
    }
    Arc::from(path.replace('\\', "/").as_str())
}

impl ModuleId {
    pub fn core(spec: &str) -> Self {
        Self::Core(Arc::from(spec))
    }

    pub fn std_module(spec: &str) -> Self {
        Self::Std(Arc::from(spec))
    }

    /// Smart constructor: routes core: → Core, std: → Std.
    pub fn stdlib(spec: &str) -> Self {
        if spec.starts_with("core:") {
            Self::Core(Arc::from(spec))
        } else {
            Self::Std(Arc::from(spec))
        }
    }

    pub fn runtime(spec: &str) -> Self {
        Self::Runtime(Arc::from(spec))
    }

    pub fn local(path: impl AsRef<Path>) -> Self {
        Self::Local(normalize_local_path(
            &path.as_ref().to_string_lossy(),
        ))
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

    /// Parse a canonical key string back into a ModuleId.
    /// Inverse of `as_str()` for core/std/runtime/local forms.
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

    /// Returns the canonical scheme this module belongs to.
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

    /// True for both Core and Std — any managed stdlib module.
    pub fn is_stdlib(&self) -> bool {
        matches!(self, Self::Core(_) | Self::Std(_))
    }

    pub fn is_runtime(&self) -> bool {
        matches!(self, Self::Runtime(_))
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

    /// std:* — public standard library.
    Stdlib(Arc<str>),

    /// core:* — intrinsic domain (compiler-internal; blocked for user code).
    Core(Arc<str>),

    /// runtime:* — ABI boundary module, resolved to native ops.
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

    /// True for both core: and std: specifiers.
    pub fn is_managed(&self) -> bool {
        matches!(self, Self::Stdlib(_) | Self::Core(_))
    }
}
