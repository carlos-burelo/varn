use std::path::PathBuf;
use std::sync::OnceLock;

use crate::spec::ModuleSpec;

pub trait StdlibProvider: Send + Sync {
    fn spec_for(&self, specifier: &str) -> Option<&'static ModuleSpec>;
    fn embedded_source(&self, specifier: &str) -> Option<&'static str>;
    fn source_path(&self, specifier: &str) -> Option<PathBuf>;
    fn all_specs(&self) -> &'static [ModuleSpec];
}

static PROVIDER: OnceLock<&'static dyn StdlibProvider> = OnceLock::new();

pub fn register(provider: &'static dyn StdlibProvider) {
    let _ = PROVIDER.set(provider);
}

pub fn get() -> Option<&'static dyn StdlibProvider> {
    PROVIDER.get().copied()
}
