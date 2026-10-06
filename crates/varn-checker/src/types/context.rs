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

    fn symbol_origin(&self, _name: &str) -> Option<varn_core::Atom> {
        None
    }
    fn source_file(&self) -> Option<&str>;

    fn get_alias_node(&self, _name: &str) -> Option<(Vec<String>, varn_core::ast::TypeNode)> {
        None
    }

    fn resolve_type_alias(&self, _name: &str, _origin: Option<&str>) -> Option<Type> {
        None
    }

    fn get_extension_method(&self, _type_name: &str, _method_name: &str) -> Option<Type> {
        None
    }

    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        None
    }

    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        None
    }

    fn ast_arena(&self) -> Option<&varn_core::ast::AstArena> {
        None
    }

    fn ty_table(&self) -> Option<&CheckerTyTable> {
        None
    }

    fn atom_text(&self, atom: varn_core::Atom) -> Option<String> {
        self.interner()
            .and_then(|i| i.try_resolve(atom))
            .or_else(|| self.ty_table().and_then(|t| t.name(atom)))
            .map(str::to_owned)
    }
}
