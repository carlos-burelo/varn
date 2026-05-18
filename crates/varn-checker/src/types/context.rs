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
    fn resolve_symbol(&self, name: &str) -> Option<Type>;
    fn source_file(&self) -> Option<&str>;

    fn get_alias_node(&self, _name: &str) -> Option<(Vec<String>, varn_core::ast::TypeNode)> {
        None
    }
}
