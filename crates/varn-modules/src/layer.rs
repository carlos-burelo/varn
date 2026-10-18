#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Core,

    Runtime,

    Std,

    User,
}

impl Layer {
    pub fn of_module(module: &str) -> Layer {
        match varn_core::ModuleKind::of(module) {
            Some(varn_core::ModuleKind::Core) => Layer::Core,
            Some(varn_core::ModuleKind::Runtime) => Layer::Runtime,
            Some(varn_core::ModuleKind::Stdlib) => Layer::Std,
            None if crate::std_root::in_source_tree(module) => Layer::Std,
            None => Layer::User,
        }
    }
}

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
