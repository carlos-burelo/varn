use std::path::{Path, PathBuf};

use crate::registry::{is_known, spec_for, MODULE_REGISTRY};
use varn_modules::spec::{ModuleKind, ModuleSpec};

const ENV_VARN_STDLIB: &str = "VARN_STDLIB";
const ENV_VARN_HOME: &str = "VARN_HOME";
const BUILTINS_DIR_NAME: &str = "varn-builtins";
const VARN_HOME_STDLIB_SUBDIR: &str = "stdlib";

pub struct CoreSourceLocator {
    stdlib_root: PathBuf,
}

impl CoreSourceLocator {
    pub fn new(stdlib_root: PathBuf) -> Self {
        Self { stdlib_root }
    }

    pub fn from_env() -> Self {
        let candidates = stdlib_candidates();
        for c in &candidates {
            if c.is_dir() {
                return Self::new(c.clone());
            }
        }
        let selected = candidates
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from(BUILTINS_DIR_NAME));
        Self::new(selected)
    }

    pub fn stdlib_root(&self) -> &Path {
        &self.stdlib_root
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

fn stdlib_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    out.push(PathBuf::from("."));

    if let Ok(v) = std::env::var(ENV_VARN_STDLIB) {
        out.push(PathBuf::from(v));
    }

    if let Ok(home) = std::env::var(ENV_VARN_HOME) {
        out.push(PathBuf::from(home).join(VARN_HOME_STDLIB_SUBDIR));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.parent().and_then(|p| p.parent()) {
            out.push(root.join(BUILTINS_DIR_NAME));
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd.join(BUILTINS_DIR_NAME));
    }

    out
}
