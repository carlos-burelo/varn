use super::BindResult;
use crate::scope::ScopeId;
use crate::types::TypeContext;
use crate::types::{ClassMemberInfo, Type};
use std::sync::Arc;
use varn_core::ast::TypeNode;

pub struct BindView<'r> {
    pub bind: &'r BindResult,
    pub resolver: &'r dyn crate::resolver::ImportResolver,
    pub scope: ScopeId,
}

impl<'r> BindView<'r> {
    pub fn new(bind: &'r BindResult, resolver: &'r dyn crate::resolver::ImportResolver) -> Self {
        Self {
            bind,
            resolver,
            scope: bind.global_scope,
        }
    }

    pub fn at(mut self, scope: ScopeId) -> Self {
        self.scope = scope;
        self
    }

    pub fn class_parent(&self, name: &str, origin: Option<&str>) -> Option<super::ClassParent> {
        match self.foreign(origin) {
            Some(rb) => rb.get_class_parent(name).cloned(),
            None => self.bind.get_class_parent(name).cloned(),
        }
    }

    pub fn heritage(&self, name: &str, origin: Option<&str>) -> Vec<super::ClassParent> {
        let owner = self.foreign(origin);
        let bind = owner.as_deref().unwrap_or(self.bind);
        bind.get_class_parent(name)
            .into_iter()
            .chain(bind.get_class_interfaces(name))
            .cloned()
            .collect()
    }

    pub fn type_param_names(&self, name: &str, origin: Option<&str>) -> Option<Vec<Arc<str>>> {
        let owner = self.foreign(origin);
        let bind = owner.as_deref().unwrap_or(self.bind);
        let atom = bind.interner.get(name)?;
        let id = bind
            .scopes
            .get(bind.global_scope)
            .resolve(atom, &bind.scopes)?;
        Some(
            bind.arena
                .get(id)
                .type_params
                .iter()
                .map(|p| Arc::from(bind.interner.resolve(*p)))
                .collect(),
        )
    }

    fn foreign(&self, origin: Option<&str>) -> Option<Arc<BindResult>> {
        let origin = origin?;
        if origin == self.bind.source_file.as_ref() {
            return None;
        }
        self.resolver
            .module_bind(origin)
            .or_else(|| self.resolver.stdlib_bind(origin))
    }
}

impl TypeContext for BindView<'_> {
    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        Some(&self.bind.interner)
    }

    fn ty_table(&self) -> Option<&crate::types::CheckerTyTable> {
        Some(&self.bind.ty_table)
    }

    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(rb) = self.foreign(origin) {
            return rb.get_interface_members_local(name).cloned();
        }
        self.bind.get_interface_members_local(name).cloned()
    }

    fn get_class_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(rb) = self.foreign(origin) {
            return rb.get_class_entry(name).map(|e| e.members.clone());
        }
        self.bind.get_class_entry(name).map(|e| e.members.clone())
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(rb) = self.foreign(origin) {
            return rb.get_namespace_members_local(name).cloned();
        }
        self.bind.get_namespace_members_local(name).cloned()
    }

    fn get_enum_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(rb) = self.foreign(origin) {
            return rb.get_enum_members_local(name).cloned();
        }
        self.bind.get_enum_members_local(name).cloned()
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        let scope = self.bind.scopes.get(self.scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        self.bind.arena.get(id).ty
    }

    fn generic_defaults(&self, name: &str) -> Option<Vec<Option<Type>>> {
        let scope = self.bind.scopes.get(self.scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        let sym = self.bind.arena.get(id);
        match sym.kind {
            crate::symbol::SymbolKind::Class | crate::symbol::SymbolKind::Interface => {
                (!sym.type_params.is_empty()).then(|| sym.type_param_defaults.clone())
            }
            crate::symbol::SymbolKind::Var
            | crate::symbol::SymbolKind::Let
            | crate::symbol::SymbolKind::Const
            | crate::symbol::SymbolKind::Function
            | crate::symbol::SymbolKind::TypeAlias
            | crate::symbol::SymbolKind::Enum
            | crate::symbol::SymbolKind::Parameter
            | crate::symbol::SymbolKind::Property
            | crate::symbol::SymbolKind::Method
            | crate::symbol::SymbolKind::TypeParameter
            | crate::symbol::SymbolKind::Namespace
            | crate::symbol::SymbolKind::Struct
            | crate::symbol::SymbolKind::Extension
            | crate::symbol::SymbolKind::EnumMember => None,
        }
    }

    fn symbol_origin(&self, name: &str) -> Option<varn_core::Atom> {
        let scope = self.bind.scopes.get(self.scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        self.bind.arena.get(id).origin_module
    }

    fn symbol_kind(&self, name: &str) -> Option<crate::symbol::SymbolKind> {
        let scope = self.bind.scopes.get(self.scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        Some(self.bind.arena.get(id).kind)
    }

    fn source_file(&self) -> Option<&str> {
        Some(self.bind.source_file.as_ref())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        let scope = self.bind.scopes.get(self.scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        let sym = self.bind.arena.get(id);
        let node = sym.alias_node.as_ref()?;
        Some((
            sym.type_params
                .iter()
                .map(|s| self.bind.interner.resolve(*s).to_string())
                .collect(),
            *node.clone(),
        ))
    }

    fn resolve_type_alias(&self, name: &str, origin: Option<&str>) -> Option<Type> {
        if let Some(foreign_bind) = self.foreign(origin) {
            let foreign_view = BindView::new(&foreign_bind, self.resolver);
            return foreign_view.resolve_type_alias(name, None);
        }
        let scope = self.bind.scopes.get(self.bind.global_scope);
        let atom = self.bind.interner.get(name)?;
        let id = scope.resolve(atom, &self.bind.scopes)?;
        let sym = self.bind.arena.get(id);
        if sym.kind == crate::symbol::SymbolKind::TypeAlias {
            return sym.ty;
        }
        None
    }

    fn resolver(&self) -> Option<&dyn crate::resolver::ImportResolver> {
        Some(self.resolver)
    }

    fn get_extension_method(&self, type_name: &str, method_name: &str) -> Option<Type> {
        let mangled = self
            .bind
            .extensions
            .methods
            .get(type_name)?
            .get(method_name)?;
        self.resolve_symbol(mangled)
    }
}
