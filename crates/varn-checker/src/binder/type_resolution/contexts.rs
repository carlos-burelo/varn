use crate::types::{ObjectTypeMember, Type, TypeContext};
use rustc_hash::FxHashMap;
use varn_core::ast::TypeNode;
use varn_core::TypeKind;

pub(super) struct InferBindingContext<'a> {
    pub(super) inner: Option<&'a dyn TypeContext>,
    pub(super) bindings: FxHashMap<String, Type>,
}

impl TypeContext for InferBindingContext<'_> {
    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        if let Some(ty) = self.bindings.get(name) {
            return Some(ty.clone());
        }
        self.inner.and_then(|c| c.resolve_symbol(name))
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

pub(super) fn is_member_optional(ty: &Type, key: &str, ctx: Option<&dyn TypeContext>) -> bool {
    match &ty.0 {
        TypeKind::Object(members) => members.iter().any(|m| match m {
            ObjectTypeMember::Property { name, optional, .. } => name.as_ref() == key && *optional,
            _ => false,
        }),
        TypeKind::Named(name, _) => ctx
            .and_then(|c| {
                c.get_interface_members(name, None)
                    .or_else(|| c.get_class_members(name, None))
            })
            .and_then(|members| {
                members
                    .iter()
                    .find(|m| m.name.as_ref() == key)
                    .map(|m| m.is_optional)
            })
            .unwrap_or(false),
        _ => false,
    }
}

pub(super) struct MappedContext<'a> {
    pub(super) inner: Option<&'a dyn TypeContext>,
    pub(super) key_var: String,
    pub(super) key_value: Type,
}

impl TypeContext for MappedContext<'_> {
    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        if name == self.key_var {
            return Some(self.key_value.clone());
        }
        self.inner.and_then(|c| c.resolve_symbol(name))
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
    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        if let Some(pos) = self.params.iter().position(|p| p == name) {
            return Some(self.args[pos].clone());
        }
        self.inner.and_then(|c| c.resolve_symbol(name))
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
