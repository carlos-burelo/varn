use super::provider;
use super::spec::{ModuleKind, ModuleSpec};

fn module_ids_of_kind(kind: ModuleKind) -> Vec<&'static str> {
    provider::get()
        .map(|p| {
            p.all_specs()
                .iter()
                .filter(|m| m.kind == kind)
                .map(|m| m.id)
                .collect()
        })
        .unwrap_or_default()
}

pub fn core_module_ids() -> Vec<&'static str> {
    module_ids_of_kind(ModuleKind::Core)
}

pub fn prelude_modules() -> Vec<&'static ModuleSpec> {
    provider::get()
        .map(|p| {
            p.all_specs()
                .iter()
                .filter(|m| m.kind == ModuleKind::Core && m.has_code)
                .collect()
        })
        .unwrap_or_default()
}

pub fn std_module_ids() -> Vec<&'static str> {
    module_ids_of_kind(ModuleKind::Stdlib)
}

pub fn is_known_stdlib_module(specifier: &str) -> bool {
    provider::get()
        .and_then(|p| p.spec_for(specifier))
        .is_some()
}
