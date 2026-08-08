use std::path::PathBuf;
use std::sync::OnceLock;

use crate::spec::ModuleSpec;
use crate::std_root::StdProvenance;

pub trait StdlibProvider: Send + Sync {
    fn spec_for(&self, specifier: &str) -> Option<&'static ModuleSpec>;
    fn embedded_source(&self, specifier: &str) -> Option<&'static str>;
    fn source_path(&self, specifier: &str) -> Option<PathBuf>;
    fn all_specs(&self) -> &'static [ModuleSpec];

    /// Precomputed checker interface (postcard CachedModule) from the active
    /// std bundle. None → checker parses source as before.
    fn interface_blob(&self, _specifier: &str) -> Option<&'static [u8]> {
        None
    }
    /// Precompiled FunctionProto (postcard) from the active std bundle.
    /// None → loader compiles source as before.
    fn bytecode_blob(&self, _specifier: &str) -> Option<&'static [u8]> {
        None
    }

    /// Original `.vn` text of a bundled std module, for editor features only.
    ///
    /// Deliberately not folded into [`Self::embedded_source`]: that one means
    /// "compile this module from this text", and drives both the recompilation
    /// graph and `.vnc` invalidation. Blob-backed modules must stay out of
    /// those — the bundle is their unit of verification. Serving them here
    /// keeps goto-definition working without dragging them back into the
    /// compile path.
    fn bundled_source(&self, _specifier: &str) -> Option<&'static str> {
        None
    }
    /// (description, provenance) of the active std, for diagnostics.
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
