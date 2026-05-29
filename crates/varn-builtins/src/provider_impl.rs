use std::path::PathBuf;
use varn_modules::provider::StdlibProvider;
use varn_modules::spec::ModuleSpec;

use crate::loader::CoreSourceLocator;
use crate::registry::{spec_for, MODULE_REGISTRY};

struct BuiltinsProvider;

impl StdlibProvider for BuiltinsProvider {
    fn spec_for(&self, specifier: &str) -> Option<&'static ModuleSpec> {
        spec_for(specifier)
    }

    fn embedded_source(&self, specifier: &str) -> Option<&'static str> {
        spec_for(specifier)?.source()
    }

    fn source_path(&self, specifier: &str) -> Option<PathBuf> {
        CoreSourceLocator::from_env().vn_source_path(specifier)
    }

    fn all_specs(&self) -> &'static [ModuleSpec] {
        MODULE_REGISTRY
    }
}

static PROVIDER: BuiltinsProvider = BuiltinsProvider;

pub fn register_provider() {
    varn_modules::provider::register(&PROVIDER);
}
