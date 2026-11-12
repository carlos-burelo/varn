use std::path::PathBuf;
use std::sync::OnceLock;

use crate::spec::ModuleSpec;
use crate::std_root::StdProvenance;

#[derive(Clone, Copy)]
pub struct ProvidedModule {
    pub source: &'static str,
    pub interface: Option<&'static [u8]>,
    pub bytecode: Option<&'static [u8]>,
}

pub trait StdlibProvider: Send + Sync {
    fn spec_for(&self, specifier: &str) -> Option<&'static ModuleSpec>;
    fn module(&self, specifier: &str) -> Option<ProvidedModule>;
    fn source_path(&self, specifier: &str) -> Option<PathBuf>;
    fn all_specs(&self) -> &'static [ModuleSpec];

    fn std_provenance(&self) -> Option<(String, StdProvenance)> {
        None
    }

    fn image_identity(&self) -> u64 {
        0
    }

    fn native_op_id(&self, _module: &str, _namespace: &str, _symbol: &str) -> Option<u64> {
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
