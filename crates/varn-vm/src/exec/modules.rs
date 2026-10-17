use crate::error::{RuntimeError, VmResult};
use varn_core::ModuleId;

pub(crate) fn resolve_specifier_to_id(specifier: &str, from: &ModuleId) -> VmResult<ModuleId> {
    let from_str = from.as_str().to_string();
    let from_layer = varn_modules::layer::Layer::of_module(&from_str);
    let target_layer = varn_modules::layer::Layer::of_module(specifier);
    if matches!(target_layer, varn_modules::layer::Layer::Runtime)
        && !matches!(from_layer, varn_modules::layer::Layer::Std)
    {
        return Err(RuntimeError::new(format!(
            "E_LAYER: '{specifier}' is a host module reserved for the standard library"
        )));
    }
    varn_modules::resolver::resolve(specifier, from).map_err(RuntimeError::new)
}

pub(crate) fn resolve_specifier_from_path(
    specifier: &str,
    source_file: &str,
) -> VmResult<ModuleId> {
    let from = if source_file.is_empty() {
        ModuleId::local_str(".")
    } else {
        ModuleId::from_canonical_str(source_file)
    };
    resolve_specifier_to_id(specifier, &from)
}
