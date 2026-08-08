use std::path::{Path, PathBuf};

use crate::registry::{is_known, spec_for, MODULE_REGISTRY};
use varn_modules::spec::{ModuleKind, ModuleSpec};

/// Registry lookups over `core:`/`runtime:` modules.
///
/// `core:`/`runtime:` sources are `include_str!`-embedded, so nothing here
/// needs the filesystem — [`Self::vn_source_path`] exists only for the
/// checkout, where `vn_source` is a repo-relative path. A released binary
/// serves these through [`Self::embedded_source`], and the editor through
/// the mirror `vn lsp` writes.
pub struct CoreSourceLocator {
    stdlib_root: PathBuf,
}

impl CoreSourceLocator {
    pub fn new(stdlib_root: PathBuf) -> Self {
        Self { stdlib_root }
    }

    /// Rooted at the working directory: `vn_source` fields are
    /// `crates/varn-builtins/src/...`, which only resolve from a checkout.
    pub fn from_checkout() -> Self {
        Self::new(PathBuf::from("."))
    }

    pub fn is_known(&self, specifier: &str) -> bool {
        is_known(specifier)
    }

    pub fn spec_for(&self, specifier: &str) -> Option<&'static ModuleSpec> {
        spec_for(specifier)
    }

    pub fn is_core(&self, specifier: &str) -> bool {
        spec_for(specifier).is_some_and(|s| s.kind == ModuleKind::Core)
    }

    pub fn is_stdlib(&self, specifier: &str) -> bool {
        spec_for(specifier).is_some_and(|s| s.kind == ModuleKind::Stdlib)
    }

    pub fn embedded_source(&self, specifier: &str) -> Option<&'static str> {
        spec_for(specifier)?.source()
    }

    pub fn vn_source_path(&self, specifier: &str) -> Option<PathBuf> {
        let spec = spec_for(specifier)?;
        let path = self.stdlib_root.join(spec.vn_source);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    pub fn core_modules(&self) -> impl Iterator<Item = &'static ModuleSpec> {
        MODULE_REGISTRY
            .iter()
            .filter(|m| m.kind == ModuleKind::Core)
    }

    pub fn stdlib_modules(&self) -> impl Iterator<Item = &'static ModuleSpec> {
        MODULE_REGISTRY
            .iter()
            .filter(|m| m.kind == ModuleKind::Stdlib)
    }

    pub fn resolve_relative(&self, base_dir: &Path, specifier: &str) -> Option<String> {
        let base = if base_dir.is_file() {
            base_dir.parent().unwrap_or(base_dir)
        } else {
            base_dir
        };
        let resolved = base.join(specifier);
        let normalized = resolved.components().fold(PathBuf::new(), |mut acc, c| {
            use std::path::Component::*;
            match c {
                ParentDir => {
                    acc.pop();
                }
                CurDir => {}
                c => acc.push(c),
            }
            acc
        });
        Some(normalized.to_string_lossy().to_string())
    }
}
