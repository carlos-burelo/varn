use varn_modules::spec::{ModuleKind, ModuleSpec};

include!(concat!(env!("OUT_DIR"), "/registry.generated.rs"));

pub fn is_known(specifier: &str) -> bool {
    MODULE_REGISTRY.iter().any(|m| m.id == specifier)
}

pub fn spec_for(specifier: &str) -> Option<&'static ModuleSpec> {
    MODULE_REGISTRY.iter().find(|m| m.id == specifier)
}
