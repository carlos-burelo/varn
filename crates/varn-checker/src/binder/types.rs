use crate::builtins::loader::BuiltinMembers;
use crate::module_resolver::resolve_module_bind_ref;
use crate::scope::{ScopeArena, ScopeId};
use crate::symbol::{Symbol, SymbolArena, SymbolId};
use crate::types::{ClassMemberInfo, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{Expr, Stmt, TypeNode};

pub use crate::types::TypeContext;

#[derive(Clone)]
pub enum PendingEnrich {
    Var {
        sym_id: SymbolId,
        init: *const Expr,
    },
    Fn {
        sym_id: SymbolId,
        body: *const Stmt,
        is_async: bool,
    },
    Method {
        class_name: Rc<str>,
        key: Rc<str>,
        body: *const Stmt,
        is_async: bool,
    },
    Getter {
        class_name: Rc<str>,
        key: Rc<str>,
        body: *const Stmt,
    },
    Setter {
        class_name: Rc<str>,
        key: Rc<str>,
        body: *const Stmt,
    },
}

unsafe impl Send for PendingEnrich {}
unsafe impl Sync for PendingEnrich {}

#[derive(Clone, Default)]
pub struct TypeMembers {
    pub classes: FxHashMap<Rc<str>, ClassMemberInfo>,
    pub interfaces: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub objects: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub enums: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub namespaces: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub flattened: FxHashMap<Rc<str>, Vec<ClassMemberInfo>>,
    pub getters: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    pub setters: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
}

#[derive(Clone, Default)]
pub struct Extensions {
    pub methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Rc<str>>>,
    pub getters: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Rc<str>>>,
    pub setters: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Rc<str>>>,
}

#[derive(Clone)]
pub struct BindResult {
    pub arena: SymbolArena,
    pub scopes: ScopeArena,
    pub global_scope: ScopeId,
    pub diagnostics: varn_core::DiagnosticBag,
    pub class_methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    pub type_members: TypeMembers,
    pub class_parents: FxHashMap<Rc<str>, Rc<str>>,
    pub source_file: Rc<str>,
    pub sum_type_variants: FxHashMap<Rc<str>, Vec<Rc<str>>>,
    pub sum_variant_parent: FxHashMap<Rc<str>, Rc<str>>,
    pub sum_variant_fields: FxHashMap<Rc<str>, Vec<(Rc<str>, Type)>>,
    pub extensions: Extensions,
    pub builtin: Option<Rc<BuiltinMembers>>,
    pub pending_enrich: Vec<PendingEnrich>,
}

impl BindResult {
    pub fn global_symbols(&self) -> impl Iterator<Item = &Symbol> {
        let scope = self.scopes.get(self.global_scope);
        scope.ordered.iter().map(|&id| self.arena.get(id))
    }

    pub fn get_class_entry(&self, name: &str) -> Option<&ClassMemberInfo> {
        self.type_members.classes.get(name).or_else(|| {
            self.builtin
                .as_ref()
                .and_then(|b| b.class_members.get(name))
        })
    }

    pub fn get_interface_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.interfaces.get(name).or_else(|| {
            self.builtin
                .as_ref()
                .and_then(|b| b.interface_members.get(name))
        })
    }

    pub fn get_namespace_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.namespaces.get(name).or_else(|| {
            self.builtin
                .as_ref()
                .and_then(|b| b.namespace_members.get(name))
        })
    }

    pub fn get_enum_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members
            .enums
            .get(name)
            .or_else(|| self.builtin.as_ref().and_then(|b| b.enum_members.get(name)))
    }

    pub fn get_class_methods_for(&self, name: &str) -> Option<&FxHashMap<Rc<str>, Type>> {
        self.class_methods.get(name).or_else(|| {
            self.builtin
                .as_ref()
                .and_then(|b| b.class_methods.get(name))
        })
    }

    pub fn get_class_parent(&self, name: &str) -> Option<&str> {
        self.class_parents
            .get(name)
            .map(|s| s.as_ref())
            .or_else(|| {
                self.builtin
                    .as_ref()
                    .and_then(|b| b.class_parents.get(name))
                    .map(|s| s.as_ref())
            })
    }

    pub fn get_flattened_members(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.flattened.get(name).or_else(|| {
            self.builtin
                .as_ref()
                .and_then(|b| b.flattened_members.get(name))
        })
    }

    pub fn has_named_type(&self, name: &str) -> bool {
        self.type_members.classes.contains_key(name)
            || self.type_members.interfaces.contains_key(name)
            || self.type_members.namespaces.contains_key(name)
            || self.type_members.enums.contains_key(name)
            || self
                .builtin
                .as_ref()
                .map(|b| {
                    b.class_members.contains_key(name)
                        || b.interface_members.contains_key(name)
                        || b.namespace_members.contains_key(name)
                        || b.enum_members.contains_key(name)
                })
                .unwrap_or(false)
    }
}

impl TypeContext for BindResult {
    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = resolve_module_bind_ref(origin) {
                    return rb.get_interface_members_local(name).cloned();
                }
            }
        }
        self.get_interface_members_local(name).cloned()
    }

    fn get_class_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = resolve_module_bind_ref(origin) {
                    return rb.get_class_entry(name).map(|e| e.members.clone());
                }
            }
        }
        self.get_class_entry(name).map(|e| e.members.clone())
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = resolve_module_bind_ref(origin) {
                    return rb.get_namespace_members_local(name).cloned();
                }
            }
        }
        self.get_namespace_members_local(name).cloned()
    }

    fn get_enum_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = resolve_module_bind_ref(origin) {
                    return rb.get_enum_members_local(name).cloned();
                }
            }
        }
        self.get_enum_members_local(name).cloned()
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        let scope = self.scopes.get(self.global_scope);
        let id = scope.resolve(name, &self.scopes)?;
        self.arena.get(id).ty.clone()
    }

    fn source_file(&self) -> Option<&str> {
        Some(self.source_file.as_ref())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        let scope = self.scopes.get(self.global_scope);
        let id = scope.resolve(name, &self.scopes)?;
        let sym = self.arena.get(id);
        let node = sym.alias_node.as_ref()?;
        Some((
            sym.type_params.iter().map(|s| s.to_string()).collect(),
            *node.clone(),
        ))
    }
}
