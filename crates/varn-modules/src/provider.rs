use std::path::PathBuf;
use std::sync::OnceLock;

use crate::spec::ModuleSpec;
use crate::std_root::StdProvenance;

pub trait StdlibProvider: Send + Sync {
    fn spec_for(&self, specifier: &str) -> Option<&'static ModuleSpec>;
    fn embedded_source(&self, specifier: &str) -> Option<&'static str>;
    fn source_path(&self, specifier: &str) -> Option<PathBuf>;
    fn all_specs(&self) -> &'static [ModuleSpec];

    fn interface_blob(&self, _specifier: &str) -> Option<&'static [u8]> {
        None
    }

    fn bytecode_blob(&self, _specifier: &str) -> Option<&'static [u8]> {
        None
    }

    fn bundled_source(&self, _specifier: &str) -> Option<&'static str> {
        None
    }

    fn std_provenance(&self) -> Option<(String, StdProvenance)> {
        None
    }
}

static PROVIDER: OnceLock<&'static dyn StdlibProvider> = OnceLock::new();

pub fn register(provider: &'static dyn StdlibProvider) {
    let _ = PROVIDER.set(provider);
}

pub fn get() -> Option<&'static dyn StdlibProvider> {
    PROVIDER.get().copied()
}
