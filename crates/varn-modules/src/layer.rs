//! The module layers of ADR-0018 and the one rule for which layer may import
//! which. Every phase that sees an import (binder, loader, bundle builder)
//! asks here, so the rule cannot drift between them.

/// A module's layer, from its id (`core:`, `runtime:`, `std:`) or, for a file
/// of the std source tree, from where it lives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// Language builtins: always in scope, never imported by name.
    Core,
    /// Host natives backing the standard library.
    Runtime,
    /// The standard library: imported explicitly.
    Std,
    /// User code and packages.
    User,
}

impl Layer {
    /// The layer a module named by id or file path belongs to.
    pub fn of_module(module: &str) -> Layer {
        if module.starts_with(crate::spec::CORE_PREFIX) {
            Layer::Core
        } else if module.starts_with(crate::RUNTIME_PREFIX) {
            Layer::Runtime
        } else if module.starts_with(crate::spec::STD_PREFIX)
            || crate::std_root::in_source_tree(module)
        {
            Layer::Std
        } else {
            Layer::User
        }
    }
}

/// Whether a module of layer `from` may import `specifier`; `Err` carries the
/// diagnostic message. A relative import stays inside its importer's tree, so
/// it never crosses a layer.
pub fn check_import(from: Layer, specifier: &str) -> Result<(), String> {
    if matches!(
        varn_core::ImportSpecifier::parse(specifier),
        varn_core::ImportSpecifier::Relative(_)
    ) {
        return Ok(());
    }
    match (Layer::of_module(specifier), from) {
        (Layer::Core, Layer::Core)
        | (Layer::Runtime, Layer::Std)
        | (Layer::Std, Layer::Std | Layer::User)
        | (Layer::User, Layer::User) => Ok(()),
        (Layer::Core, Layer::Runtime | Layer::Std | Layer::User) => Err(format!(
            "'{specifier}' is built into the language and always in scope; remove the import"
        )),
        (Layer::Runtime, Layer::Core | Layer::Runtime | Layer::User) => Err(format!(
            "'{specifier}' is a host module reserved for the standard library; import its 'std:' counterpart"
        )),
        (Layer::Std, Layer::Core | Layer::Runtime) => Err(format!(
            "'{specifier}' is a standard library module; the {from:?} layer cannot depend on it"
        )),
        (Layer::User, Layer::Core | Layer::Runtime | Layer::Std) => Err(format!(
            "'{specifier}' is a package; the {from:?} layer cannot depend on it"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_matrix() {
        assert!(check_import(Layer::User, "std:fs").is_ok());
        assert!(check_import(Layer::User, "core:types").is_err());
        assert!(check_import(Layer::User, "runtime:sys").is_err());
        assert!(check_import(Layer::Std, "runtime:sys").is_ok());
        assert!(check_import(Layer::Std, "core:types").is_err());
        assert!(check_import(Layer::Core, "core:types/int").is_ok());
        assert!(check_import(Layer::Core, "std:fs").is_err());
        assert!(check_import(Layer::Std, "./sibling").is_ok());
        assert!(check_import(Layer::Std, "C:/work/app.vn").is_ok());
    }
}
