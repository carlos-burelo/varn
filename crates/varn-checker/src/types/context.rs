use super::*;

pub trait TypeContext {
    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>>;
    fn get_class_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>>;
    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>>;
    fn get_enum_members(&self, _name: &str, _origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        None
    }
    fn resolve_symbol(&self, name: &str) -> Option<Type>;
    fn source_file(&self) -> Option<&str>;

    fn get_alias_node(&self, _name: &str) -> Option<(Vec<String>, varn_core::ast::TypeNode)> {
        None
    }

    fn get_extension_method(&self, _type_name: &str, _method_name: &str) -> Option<Type> {
        None
    }

    /// How this context reaches other modules, when it can.
    ///
    /// Type resolution has to expand generic aliases declared in `core:types`,
    /// which means resolving a module from inside the type resolver. Exposing
    /// it here keeps that a property of the context that was passed in, rather
    /// than something the resolver reaches for globally.
    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        None
    }
}
