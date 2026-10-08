use crate::types::{CheckerTyTable, ObjectTypeMember, Type, TypeContext};
use rustc_hash::FxHashMap;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

pub(super) struct InferBindingContext<'a> {
    pub(super) inner: Option<&'a dyn TypeContext>,
    pub(super) bindings: FxHashMap<String, Type>,
}

impl TypeContext for InferBindingContext<'_> {
    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        self.inner.and_then(|c| c.resolver())
    }

    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        self.inner.and_then(|c| c.interner())
    }

    fn ty_table(&self) -> Option<&CheckerTyTable> {
        self.inner.and_then(|c| c.ty_table())
    }

    fn ast_arena(&self) -> Option<&varn_core::ast::AstArena> {
        self.inner.and_then(|c| c.ast_arena())
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        if let Some(ty) = self.bindings.get(name) {
            return Some(*ty);
        }
        self.inner.and_then(|c| c.resolve_symbol(name))
    }

    fn symbol_origin(&self, name: &str) -> Option<varn_core::Atom> {
        self.inner.and_then(|c| c.symbol_origin(name))
    }

    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner
            .and_then(|c| c.get_interface_members(name, origin))
    }

    fn get_class_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner.and_then(|c| c.get_class_members(name, origin))
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner
            .and_then(|c| c.get_namespace_members(name, origin))
    }

    fn source_file(&self) -> Option<&str> {
        self.inner.and_then(|c| c.source_file())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        self.inner.and_then(|c| c.get_alias_node(name))
    }
}

pub(super) fn is_member_optional(
    ty: &Type,
    key: &str,
    ctx: Option<&dyn TypeContext>,
    table: &CheckerTyTable,
) -> bool {
    match table.get(ty.0) {
        TypeKind::Object(mid) => table.get_object_members(mid).iter().any(|m| match m {
            ObjectTypeMember::Property { name, optional, .. } => name.as_ref() == key && *optional,
            ObjectTypeMember::Method { .. } | ObjectTypeMember::Index { .. } | ObjectTypeMember::Callable { .. } => false,
        }),
        TypeKind::Named(name, _) => {
            let name = ctx.and_then(|c| c.interner()).map(|i| i.resolve(name));
            name.and_then(|name| {
                ctx.and_then(|c| {
                    c.get_interface_members(name, None)
                        .or_else(|| c.get_class_members(name, None))
                })
                .and_then(|members| {
                    members
                        .iter()
                        .find(|m| m.name.as_ref() == key)
                        .map(|m| m.is_optional)
                })
            })
            .unwrap_or(false)
        }
        TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_) | TypeKind::This | TypeKind::Array(_) | TypeKind::Union(_) | TypeKind::Intersection(_) | TypeKind::Tuple(_) | TypeKind::Generic(..) | TypeKind::TemplateLiteral(_) | TypeKind::Fn(_) | TypeKind::Typeof(_) | TypeKind::KeyOf(_) | TypeKind::IndexedAccess { .. } | TypeKind::Mapped { .. } | TypeKind::Conditional { .. } | TypeKind::Infer(_) | TypeKind::EnumVariant { .. } | TypeKind::TypePredicate { .. } => false,
    }
}

pub(super) struct MappedContext<'a> {
    pub(super) inner: Option<&'a dyn TypeContext>,
    pub(super) key_var: String,
    pub(super) key_value: Type,
}

impl TypeContext for MappedContext<'_> {
    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        self.inner.and_then(|c| c.resolver())
    }

    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        self.inner.and_then(|c| c.interner())
    }

    fn ty_table(&self) -> Option<&CheckerTyTable> {
        self.inner.and_then(|c| c.ty_table())
    }

    fn ast_arena(&self) -> Option<&varn_core::ast::AstArena> {
        self.inner.and_then(|c| c.ast_arena())
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        if name == self.key_var {
            return Some(self.key_value);
        }
        self.inner.and_then(|c| c.resolve_symbol(name))
    }

    fn symbol_origin(&self, name: &str) -> Option<varn_core::Atom> {
        self.inner.and_then(|c| c.symbol_origin(name))
    }

    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner
            .and_then(|c| c.get_interface_members(name, origin))
    }

    fn get_class_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner.and_then(|c| c.get_class_members(name, origin))
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner
            .and_then(|c| c.get_namespace_members(name, origin))
    }

    fn source_file(&self) -> Option<&str> {
        self.inner.and_then(|c| c.source_file())
    }
}

pub(super) struct AliasSubstitutionContext<'a> {
    pub(super) inner: Option<&'a dyn TypeContext>,
    pub(super) params: Vec<String>,
    pub(super) args: Vec<Type>,
}

impl TypeContext for AliasSubstitutionContext<'_> {
    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        self.inner.and_then(|c| c.resolver())
    }

    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        self.inner.and_then(|c| c.interner())
    }

    fn ty_table(&self) -> Option<&CheckerTyTable> {
        self.inner.and_then(|c| c.ty_table())
    }

    fn ast_arena(&self) -> Option<&varn_core::ast::AstArena> {
        None
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        if let Some(pos) = self.params.iter().position(|p| p == name) {
            return Some(self.args[pos]);
        }
        self.inner.and_then(|c| c.resolve_symbol(name))
    }

    fn symbol_origin(&self, name: &str) -> Option<varn_core::Atom> {
        self.inner.and_then(|c| c.symbol_origin(name))
    }

    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner
            .and_then(|c| c.get_interface_members(name, origin))
    }

    fn get_class_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner.and_then(|c| c.get_class_members(name, origin))
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<crate::types::ClassMemberInfo>> {
        self.inner
            .and_then(|c| c.get_namespace_members(name, origin))
    }

    fn source_file(&self) -> Option<&str> {
        self.inner.and_then(|c| c.source_file())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        self.inner.and_then(|c| c.get_alias_node(name))
    }
}
