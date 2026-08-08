use crate::error::{RuntimeError, VmResult};
use varn_core::ModuleId;

pub(crate) fn resolve_specifier_to_id(specifier: &str, from: &ModuleId) -> VmResult<ModuleId> {
    varn_modules::resolver::ModuleResolver::new()
        .resolve(specifier, from)
        .map_err(RuntimeError::new)
}

pub(crate) fn resolve_specifier_from_path(specifier: &str, source_file: &str) -> VmResult<ModuleId> {
    let from = if source_file.is_empty() {
        ModuleId::local_str(".")
    } else {
        ModuleId::from_canonical_str(source_file)
    };
    resolve_specifier_to_id(specifier, &from)
}

