use rustc_hash::FxHashMap;
use std::sync::Arc;
use vn_core::ast::AstArena;
use vn_sem::bind::{ClassParent, Extensions, PendingEnrich, TypeMembers};
use vn_sem::scope::{ScopeArena, ScopeId};
use vn_sem::symbol::SymbolArena;
use vn_sem::types::Type;

mod binder_entry;
mod binder_intern;
mod binder_stmts;
mod binder_types;
mod binding_types;
mod class;
mod class_body;
mod class_ctor;
mod class_member;
mod context;
pub mod core;
mod decl_values;
mod decls;
mod decls_fn;
pub(crate) mod decorator_attrs;
mod definite_field_assignment;
mod diagnostics;
mod forwarder;
mod heritage;
mod imports;
mod interface;
pub mod paths;
mod regex_hygiene;
mod type_names;
mod variance;

pub(crate) use binding_types::ParamSite;
use vn_sem::resolver::ImportResolver;

pub struct Binder<'r> {
    pub(crate) resolver: &'r dyn ImportResolver,

    pub(crate) ast_arena: &'r AstArena,
    pub(crate) arena: SymbolArena,
    pub(crate) scopes: ScopeArena,
    pub(crate) current: ScopeId,
    pub(crate) class_methods: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
    pub(crate) type_members: TypeMembers,
    pub(crate) class_parents: FxHashMap<Arc<str>, ClassParent>,
    pub(crate) class_interfaces: FxHashMap<Arc<str>, Vec<ClassParent>>,
    pub(crate) diagnostics: vn_core::DiagnosticBag,
    pub(crate) interner: vn_core::AtomInterner,
    pub(crate) deps: Vec<Arc<str>>,
    pub(crate) annotation_types: FxHashMap<vn_core::ast::AstId, vn_sem::types::Type>,
    pub(crate) match_arm_scopes: FxHashMap<vn_core::ast::AstId, Vec<ScopeId>>,
    pub(crate) ty_table: std::sync::Arc<vn_sem::types::CheckerTyTable>,
    pub(crate) source_file: Arc<str>,
    pub(crate) decl_kinds: FxHashMap<Arc<str>, vn_sem::bind::DeclKind>,
    pub(crate) sum_type_variants: FxHashMap<Arc<str>, Vec<Arc<str>>>,
    pub(crate) sum_variant_parent: FxHashMap<Arc<str>, Arc<str>>,
    pub(crate) sum_variant_fields: FxHashMap<Arc<str>, Vec<(Arc<str>, Type)>>,
    pub(crate) extensions: Extensions,
    pub(crate) pending_enrich: Vec<PendingEnrich>,
    reported_type_forms: rustc_hash::FxHashSet<u32>,
    reported_params: rustc_hash::FxHashSet<u32>,

    pub(crate) type_decls: FxHashMap<Arc<str>, (vn_sem::scope::ScopeId, vn_core::SourceRange)>,

    pub(crate) pending_decorator_roles: Vec<(
        vn_sem::symbol::SymbolId,
        Vec<vn_core::ast::Decorator>,
        vn_sem::scope::ScopeId,
    )>,

    pub(crate) user_decorators: rustc_hash::FxHashSet<u32>,
    pub(crate) namespace_imports: Vec<vn_sem::bind::NamespaceImport>,
    pub(crate) imported_forwards: rustc_hash::FxHashMap<Arc<str>, Arc<vn_sem::symbol::Forward>>,
    pub(crate) namespace_path: Vec<Arc<str>>,
    pub(crate) declared_type_names: FxHashMap<vn_core::ast::AstId, Arc<str>>,
    pub(crate) generic_type_uses: Vec<vn_sem::bind::GenericTypeUse>,
}
