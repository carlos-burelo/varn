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
pub use inference_utils::build_fn_type;
pub use type_inference::{infer_expr_type, pattern_lead_name, widen_literal};
pub use type_resolution::{resolve_primitive, resolve_type_node};
pub use types::{BindResult, BindView, Extensions, PendingEnrich, TypeMembers};

pub struct Binder<'r> {
    /// How this binder reaches other modules. Borrowed, not owned: the
    /// resolver constructs binders while binding a module's imports, so an
    /// owning handle would make the ownership circular.
    pub(crate) resolver: &'r dyn ImportResolver,
    /// The parsed program's expression/statement nodes (fase1-componente2:
    /// `Expr`/`Stmt` are no longer owned trees — every AST node the binder
    /// visits is an `ExprId`/`StmtId` resolved against this arena). Named
    /// `ast_arena` (not `arena`) to avoid colliding with the symbol arena
    /// below, which every binder method already calls `self.arena`.
    pub(crate) ast_arena: &'r AstArena,
    pub(crate) arena: SymbolArena,
    pub(crate) scopes: ScopeArena,
    pub(crate) current: ScopeId,
    pub(crate) class_methods: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
    pub(crate) type_members: TypeMembers,
    pub(crate) class_parents: FxHashMap<Arc<str>, Arc<str>>,
    pub(crate) diagnostics: varn_core::DiagnosticBag,
    /// The real per-parse `AtomInterner`, threaded in from `Binder::bind`'s
    /// caller (see the doc comment on `BindResult::interner`).
    pub(crate) interner: varn_core::AtomInterner,
    /// The shared, per-compilation `CheckerTyId` table — same lifecycle as
    /// `interner` above: snapshotted from `ImportResolver::ty_table_snapshot`
    /// when this `Binder` is constructed, grown while binding, published
    /// back via `ImportResolver::set_ty_table` by whoever drives binding
    /// (mirrors `DiskResolver::bind_and_cache`'s `set_interner` call), and
    /// carried out to `BindResult::ty_table` so later checking/emit stages
    /// read the same ids this bind minted.
    pub(crate) ty_table: std::sync::Arc<crate::types::CheckerTyTable>,
    pub(crate) source_file: Arc<str>,
    pub(crate) sum_type_variants: FxHashMap<Arc<str>, Vec<Arc<str>>>,
    pub(crate) sum_variant_parent: FxHashMap<Arc<str>, Arc<str>>,
    pub(crate) sum_variant_fields: FxHashMap<Arc<str>, Vec<(Arc<str>, Type)>>,
    pub(crate) extensions: Extensions,
    pub(crate) pending_enrich: Vec<PendingEnrich>,
    reported_type_forms: rustc_hash::FxHashSet<u32>,
    pub(crate) array_watch: Vec<array_evolve::ArrayCandidate>,
    /// Optimization-only element types proved for evolving empty-array
    /// locals (Task A0.3'); moved into `BindResult::evolved_array_types`.
    pub(crate) evolved_array_types: FxHashMap<u32, Type>,
    /// Where each class and enum name was first declared (see `type_names`).
    pub(crate) type_decls: FxHashMap<Arc<str>, (crate::scope::ScopeId, varn_core::SourceRange)>,
}
