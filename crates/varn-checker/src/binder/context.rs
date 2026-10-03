use super::Binder;
use crate::types::{ClassMemberInfo, Type, TypeContext};
use varn_core::ast::TypeNode;

impl TypeContext for Binder<'_> {
    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        Some(self.resolver)
    }

    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        Some(&self.interner)
    }

    fn ty_table(&self) -> Option<&crate::types::CheckerTyTable> {
        Some(&self.ty_table)
    }

    fn ast_arena(&self) -> Option<&varn_core::ast::AstArena> {
        Some(self.ast_arena)
    }

    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = self
                    .resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))
                {
                    return rb.type_members.interfaces.get(name).cloned();
                }
            }
        }
        self.type_members.interfaces.get(name).cloned()
    }

    fn get_class_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = self
                    .resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))
                {
                    return rb.type_members.classes.get(name).map(|e| e.members.clone());
                }
            }
        }
        self.type_members
            .classes
            .get(name)
            .map(|e| e.members.clone())
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = self
                    .resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))
                {
                    return rb.type_members.namespaces.get(name).cloned();
                }
            }
        }
        self.type_members.namespaces.get(name).cloned()
    }

    fn get_enum_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = self
                    .resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))
                {
                    return rb.type_members.enums.get(name).cloned();
                }
            }
        }
        self.type_members.enums.get(name).cloned()
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        self.arena.get(id).ty
    }

    fn symbol_origin(&self, name: &str) -> Option<varn_core::Atom> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        self.arena.get(id).origin_module
    }

    fn source_file(&self) -> Option<&str> {
        Some(self.source_file.as_ref())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        let sym = self.arena.get(id);
        let node = sym.alias_node.as_ref()?;
        Some((
            sym.type_params
                .iter()
                .map(|s| self.interner.resolve(*s).to_string())
                .collect(),
            *node.clone(),
        ))
    }
}
