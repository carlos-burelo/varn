use super::Binder;
use varn_core::ast::TypeNode;
use varn_sem::types::{ClassMemberInfo, Type, TypeContext};

impl TypeContext for Binder<'_> {
    fn resolver(&self) -> Option<&dyn varn_sem::resolver::ImportResolver> {
        Some(self.resolver)
    }

    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        Some(&self.interner)
    }

    fn ty_table(&self) -> Option<&varn_sem::types::CheckerTyTable> {
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
        if let Some(origin) = origin
            && origin != self.source_file.as_ref()
            && let Some(rb) = self
                .resolver
                .module_bind(origin)
                .or_else(|| self.resolver.stdlib_bind(origin))
        {
            return rb.type_members.interfaces.get(name).cloned();
        }
        self.type_members.interfaces.get(name).cloned()
    }

    fn get_class_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin
            && origin != self.source_file.as_ref()
            && let Some(rb) = self
                .resolver
                .module_bind(origin)
                .or_else(|| self.resolver.stdlib_bind(origin))
        {
            return rb.type_members.classes.get(name).map(|e| e.members.clone());
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
        if let Some(origin) = origin
            && origin != self.source_file.as_ref()
            && let Some(rb) = self
                .resolver
                .module_bind(origin)
                .or_else(|| self.resolver.stdlib_bind(origin))
        {
            return rb.type_members.namespaces.get(name).cloned();
        }
        self.type_members.namespaces.get(name).cloned()
    }

    fn get_enum_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin
            && origin != self.source_file.as_ref()
            && let Some(rb) = self
                .resolver
                .module_bind(origin)
                .or_else(|| self.resolver.stdlib_bind(origin))
        {
            return rb.type_members.enums.get(name).cloned();
        }
        self.type_members.enums.get(name).cloned()
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        self.arena.get(id).ty
    }

    fn generic_defaults(&self, name: &str) -> Option<Vec<Option<Type>>> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        let sym = self.arena.get(id);
        match sym.kind {
            varn_sem::symbol::SymbolKind::Class | varn_sem::symbol::SymbolKind::Interface => {
                (!sym.type_params.is_empty()).then(|| sym.type_param_defaults.clone())
            }
            varn_sem::symbol::SymbolKind::Var
            | varn_sem::symbol::SymbolKind::Let
            | varn_sem::symbol::SymbolKind::Const
            | varn_sem::symbol::SymbolKind::Function
            | varn_sem::symbol::SymbolKind::TypeAlias
            | varn_sem::symbol::SymbolKind::Enum
            | varn_sem::symbol::SymbolKind::Parameter
            | varn_sem::symbol::SymbolKind::Property
            | varn_sem::symbol::SymbolKind::Method
            | varn_sem::symbol::SymbolKind::TypeParameter
            | varn_sem::symbol::SymbolKind::Namespace
            | varn_sem::symbol::SymbolKind::Struct
            | varn_sem::symbol::SymbolKind::Extension
            | varn_sem::symbol::SymbolKind::EnumMember => None,
        }
    }

    fn symbol_origin(&self, name: &str) -> Option<varn_core::Atom> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        self.arena.get(id).origin_module
    }

    fn symbol_kind(&self, name: &str) -> Option<varn_sem::symbol::SymbolKind> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        Some(self.arena.get(id).kind)
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
