use crate::scope::{ScopeArena, ScopeId};
use crate::symbol::SymbolArena;
use crate::types::Type;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::AstArena;

mod array_evolve;
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
mod decl_values;
mod declared_types;
mod decls;
mod decls_array;
mod decls_fn;
pub(crate) mod decorator_attrs;
mod definite_field_assignment;
mod diagnostics;
mod imports;
mod inference_utils;
mod interface;
mod type_infer_expr;
mod type_infer_member;
mod type_infer_ops;
pub(crate) mod type_inference;
mod type_names;
mod type_resolution;
mod types;

use crate::module_resolver::ImportResolver;
pub use crate::types::{ClassMemberInfo, ClassMemberKind, TypeContext};
pub(crate) use binding_types::ParamSite;
pub use inference_utils::build_fn_type;
pub use type_inference::{infer_expr_type, pattern_lead_name, widen_literal};
pub use type_resolution::{resolve_primitive, resolve_type_node};
pub use types::{BindResult, BindView, ClassParent, Extensions, PendingEnrich, TypeMembers};

pub struct Binder<'r> {
    pub(crate) resolver: &'r dyn ImportResolver,

    pub(crate) ast_arena: &'r AstArena,
    pub(crate) arena: SymbolArena,
    pub(crate) scopes: ScopeArena,
    pub(crate) current: ScopeId,
    pub(crate) class_methods: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
    pub(crate) type_members: TypeMembers,
    pub(crate) class_parents: FxHashMap<Arc<str>, ClassParent>,
    pub(crate) diagnostics: varn_core::DiagnosticBag,
    pub(crate) interner: varn_core::AtomInterner,
    pub(crate) deps: Vec<Arc<str>>,
    pub(crate) annotation_types: FxHashMap<varn_core::ast::AstId, crate::types::Type>,
    pub(crate) match_arm_scopes: FxHashMap<varn_core::ast::AstId, Vec<ScopeId>>,
    pub(crate) ty_table: std::sync::Arc<crate::types::CheckerTyTable>,
    pub(crate) source_file: Arc<str>,
    pub(crate) sum_type_variants: FxHashMap<Arc<str>, Vec<Arc<str>>>,
    pub(crate) sum_variant_parent: FxHashMap<Arc<str>, Arc<str>>,
    pub(crate) sum_variant_fields: FxHashMap<Arc<str>, Vec<(Arc<str>, Type)>>,
    pub(crate) extensions: Extensions,
    pub(crate) pending_enrich: Vec<PendingEnrich>,
    reported_type_forms: rustc_hash::FxHashSet<u32>,
    reported_params: rustc_hash::FxHashSet<u32>,
    pub(crate) array_watch: Vec<array_evolve::ArrayCandidate>,

    pub(crate) evolved_array_types: FxHashMap<u32, Type>,

    pub(crate) type_decls: FxHashMap<Arc<str>, (crate::scope::ScopeId, varn_core::SourceRange)>,

    pub(crate) pending_decorator_roles: Vec<(
        crate::symbol::SymbolId,
        Vec<varn_core::ast::Decorator>,
        crate::scope::ScopeId,
    )>,

    pub(crate) user_decorators: rustc_hash::FxHashSet<u32>,
}
