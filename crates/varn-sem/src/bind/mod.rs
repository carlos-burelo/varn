use crate::cores::CoreMembers;
use crate::scope::{ScopeArena, ScopeId};
use crate::symbol::{Symbol, SymbolArena, SymbolId};
use crate::types::{ClassMemberInfo, ClassMemberKind, Type};
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::sync::Arc;
use varn_core::ast::{ExprId, StmtId, TypeNode};
use varn_core::Atom;

pub use crate::types::TypeContext;

#[derive(Clone)]
pub enum PendingEnrich {
    Var {
        sym_id: SymbolId,
        init: ExprId,
    },
    Fn {
        sym_id: SymbolId,
        body: StmtId,
        is_async: bool,
    },
    Method {
        class_name: Atom,
        key: Atom,
        body: StmtId,
        is_async: bool,
    },
    Getter {
        class_name: Atom,
        key: Atom,
        body: StmtId,
    },
    Setter {
        class_name: Atom,
        key: Atom,
        body: StmtId,
    },
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TypeMembers {
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub classes: FxHashMap<Arc<str>, ClassMemberInfo>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub interfaces: FxHashMap<Arc<str>, Vec<ClassMemberInfo>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub objects: FxHashMap<Atom, Vec<ClassMemberInfo>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub enums: FxHashMap<Arc<str>, Vec<ClassMemberInfo>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub namespaces: FxHashMap<Arc<str>, Vec<ClassMemberInfo>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub flattened: FxHashMap<Arc<str>, Vec<ClassMemberInfo>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted_nested")]
    pub getters: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted_nested")]
    pub setters: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Extensions {
    #[serde(serialize_with = "crate::portable::canonical::sorted_nested")]
    pub methods: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted_nested")]
    pub getters: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted_nested")]
    pub setters: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Arc<str>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClassParent {
    pub name: Arc<str>,
    pub origin: Option<Arc<str>>,
    pub args: Vec<Type>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BindResult {
    pub arena: SymbolArena,
    pub scopes: ScopeArena,
    pub global_scope: ScopeId,
    #[serde(skip)]
    pub diagnostics: varn_core::DiagnosticBag,
    #[serde(skip)]
    pub interner: varn_core::AtomInterner,
    #[serde(skip, default)]
    pub ty_table: std::sync::Arc<crate::types::CheckerTyTable>,
    pub deps: Vec<Arc<str>>,
    #[serde(skip, default)]
    pub annotation_types: FxHashMap<varn_core::ast::AstId, Type>,
    #[serde(skip, default)]
    pub match_arm_scopes: FxHashMap<varn_core::ast::AstId, Vec<ScopeId>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted_nested")]
    pub class_methods: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
    pub type_members: TypeMembers,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub class_parents: FxHashMap<Arc<str>, ClassParent>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub class_interfaces: FxHashMap<Arc<str>, Vec<ClassParent>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub type_param_variance: FxHashMap<Arc<str>, Vec<crate::compat::variance::Variance>>,
    pub source_file: Arc<str>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub decl_kinds: FxHashMap<Arc<str>, DeclKind>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub sum_type_variants: FxHashMap<Arc<str>, Vec<Arc<str>>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub sum_variant_parent: FxHashMap<Arc<str>, Arc<str>>,
    #[serde(serialize_with = "crate::portable::canonical::sorted")]
    pub sum_variant_fields: FxHashMap<Arc<str>, Vec<(Arc<str>, Type)>>,
    pub extensions: Extensions,
    #[serde(skip)]
    pub core: Option<Arc<CoreMembers>>,
    #[serde(skip)]
    pub pending_enrich: Vec<PendingEnrich>,
    #[serde(skip, default)]
    pub user_decorators: FxHashSet<u32>,
    #[serde(skip, default)]
    pub namespace_imports: Vec<NamespaceImport>,
    #[serde(skip, default)]
    pub declared_type_names: FxHashMap<varn_core::ast::AstId, Arc<str>>,
}

#[derive(Clone, Debug)]
pub struct NamespaceImport {
    pub binding: Arc<str>,
    pub hang: Arc<str>,
    pub exported: Arc<str>,
    pub members: Vec<Arc<str>>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum DeclKind {
    SumType,
    Enum,
    Class,
    Interface,
    Namespace,
}

mod settle;
mod view;
pub use settle::settle_members;
pub use view::BindView;

impl BindResult {
    pub fn classify_decl(&self, name: &str) -> Option<DeclKind> {
        self.decl_kinds.get(name).copied()
    }

    pub fn declares_type(&self, name: &str) -> bool {
        self.type_members.classes.contains_key(name)
            || self.type_members.interfaces.contains_key(name)
            || self.type_members.enums.contains_key(name)
            || self.type_members.namespaces.contains_key(name)
            || self.sum_type_variants.contains_key(name)
    }

    pub fn enum_layout(&self, name: &str) -> Option<Vec<VariantLayout>> {
        let fields = |variant: &Arc<str>| {
            self.sum_variant_fields
                .get(variant)
                .cloned()
                .unwrap_or_default()
        };
        if let Some(variants) = self.sum_type_variants.get(name) {
            return Some(variants.iter().map(|v| (v.clone(), fields(v))).collect());
        }
        let members = self.type_members.enums.get(name)?;
        Some(
            members
                .iter()
                .filter(|m| {
                    !m.is_static
                        && !matches!(
                            m.kind,
                            ClassMemberKind::Method
                                | ClassMemberKind::Getter
                                | ClassMemberKind::Setter
                                | ClassMemberKind::Constructor
                        )
                })
                .map(|m| (m.name.clone(), fields(&m.name)))
                .collect(),
        )
    }
}

pub type VariantLayout = (Arc<str>, Vec<(Arc<str>, Type)>);

impl BindResult {
    pub fn global_symbols(&self) -> impl Iterator<Item = &Symbol> {
        let scope = self.scopes.get(self.global_scope);
        scope.ordered.iter().map(|&id| self.arena.get(id))
    }

    pub fn get_class_entry(&self, name: &str) -> Option<&ClassMemberInfo> {
        self.type_members
            .classes
            .get(name)
            .or_else(|| self.core.as_ref().and_then(|b| b.class_members.get(name)))
    }

    pub fn get_interface_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.interfaces.get(name).or_else(|| {
            self.core
                .as_ref()
                .and_then(|b| b.interface_members.get(name))
        })
    }

    pub fn get_namespace_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.namespaces.get(name).or_else(|| {
            self.core
                .as_ref()
                .and_then(|b| b.namespace_members.get(name))
        })
    }

    pub fn get_enum_members_local(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members
            .enums
            .get(name)
            .or_else(|| self.core.as_ref().and_then(|b| b.enum_members.get(name)))
    }

    pub fn get_class_methods_for(&self, name: &str) -> Option<&FxHashMap<Arc<str>, Type>> {
        self.class_methods
            .get(name)
            .or_else(|| self.core.as_ref().and_then(|b| b.class_methods.get(name)))
    }

    pub fn get_class_parent(&self, name: &str) -> Option<&ClassParent> {
        self.class_parents
            .get(name)
            .or_else(|| self.core.as_ref().and_then(|b| b.class_parents.get(name)))
    }

    pub fn get_class_interfaces(&self, name: &str) -> &[ClassParent] {
        self.class_interfaces
            .get(name)
            .map_or(&[][..], Vec::as_slice)
    }

    pub fn get_flattened_members(&self, name: &str) -> Option<&Vec<ClassMemberInfo>> {
        self.type_members.flattened.get(name).or_else(|| {
            self.core
                .as_ref()
                .and_then(|b| b.flattened_members.get(name))
        })
    }

    pub fn intrinsic_import_wire(&self, name: &str) -> Option<u8> {
        let scope = self.scopes.get(self.global_scope);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        self.arena.get(id).intrinsic_wire
    }

    pub fn get_alias_node_local(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        let scope = self.scopes.get(self.global_scope);
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

    pub fn has_named_type(&self, name: &str) -> bool {
        self.type_members.classes.contains_key(name)
            || self.type_members.interfaces.contains_key(name)
            || self.type_members.namespaces.contains_key(name)
            || self.type_members.enums.contains_key(name)
            || self
                .core
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
