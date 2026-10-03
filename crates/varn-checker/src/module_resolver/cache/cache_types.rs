use crate::binder::Extensions;
use crate::symbol::SymbolKind;
use crate::types::{ClassMemberKind, PortableType};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::operators::Visibility;

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct PortableSymbol {
    pub(crate) kind: SymbolKind,
    pub(crate) name: String,
    pub(crate) ty: Option<PortableType>,
    pub(crate) line: u32,
    pub(crate) col: u32,
    pub(crate) has_explicit_type: bool,
    pub(crate) is_async: bool,
    pub(crate) is_generator: bool,
    pub(crate) doc: Option<String>,
    pub(crate) type_params: Vec<String>,
    pub(crate) type_param_constraints: Vec<Option<PortableType>>,
    pub(crate) offset: u32,
    pub(crate) origin_module: Option<String>,
    pub(crate) re_export_path: Vec<String>,
    pub(crate) original_name: Option<String>,
    pub(crate) slot_idx: Option<usize>,
    pub(crate) intrinsic_wire: Option<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct PortableClassMemberInfo {
    pub(super) name: Arc<str>,
    pub(super) kind: ClassMemberKind,
    pub(super) is_async: bool,
    pub(super) is_generator: bool,
    pub(super) is_static: bool,
    pub(super) is_optional: bool,
    pub(super) line: u32,
    pub(super) col: u32,
    pub(super) offset: u32,
    pub(super) ty: PortableType,
    pub(super) members: Vec<PortableClassMemberInfo>,
    pub(super) visibility: Option<Visibility>,
    pub(super) is_abstract: bool,
    pub(super) is_readonly: bool,
    pub(super) is_override: bool,
    pub(super) is_builtin_or_intrinsic: bool,
    pub(super) symbol_id: Option<usize>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub(super) struct PortableTypeMembers {
    pub(super) classes: FxHashMap<Arc<str>, PortableClassMemberInfo>,
    pub(super) interfaces: FxHashMap<Arc<str>, Vec<PortableClassMemberInfo>>,
    pub(super) enums: FxHashMap<Arc<str>, Vec<PortableClassMemberInfo>>,
    pub(super) namespaces: FxHashMap<Arc<str>, Vec<PortableClassMemberInfo>>,
    pub(super) flattened: FxHashMap<Arc<str>, Vec<PortableClassMemberInfo>>,
    pub(super) getters: FxHashMap<Arc<str>, FxHashMap<Arc<str>, PortableType>>,
    pub(super) setters: FxHashMap<Arc<str>, FxHashMap<Arc<str>, PortableType>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct PortableModule {
    pub(super) exports: FxHashMap<String, PortableSymbol>,
    pub(super) arena: Vec<PortableSymbol>,
    pub(super) scopes: crate::scope::ScopeArena,
    pub(super) global_scope: crate::scope::ScopeId,
    pub(super) class_methods: FxHashMap<Arc<str>, FxHashMap<Arc<str>, PortableType>>,
    pub(super) type_members: PortableTypeMembers,
    pub(super) class_parents: FxHashMap<Arc<str>, Arc<str>>,
    pub(super) source_file: Arc<str>,
    pub(super) sum_type_variants: FxHashMap<Arc<str>, Vec<Arc<str>>>,
    pub(super) sum_variant_parent: FxHashMap<Arc<str>, Arc<str>>,
    pub(super) sum_variant_fields: FxHashMap<Arc<str>, Vec<(Arc<str>, PortableType)>>,
    pub(super) extensions: Extensions,
}
