pub(crate) mod completion;
mod decl_class;
mod decl_class_contracts;
mod decl_class_decorators;
mod decl_class_members;
mod decl_enum;
mod decl_fn;
mod decl_fn_builtins;
mod decl_misc;
mod decl_var;
mod decls;
pub(crate) mod decorator_purity;
pub(crate) mod decorator_receiver;
pub(crate) mod decorator_signature;
mod definite_assignment;
mod equality;
pub(crate) mod foreign_classes;
pub(crate) mod foreign_collect;
mod foreign_enums;
mod generic_boundaries;
mod generic_uses;
mod literal_fit;
mod nominal_shapes;
mod param_defaults;
mod profile;
pub(crate) mod recorder;
mod scope_records;
mod setup;
mod stages;
mod stmts;
mod type_names;
mod type_queries;

use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use vn_core::ast::Program;
use vn_core::ast::{AstArena, ExprId};
use vn_sem::output::{CheckOptions, CheckProfile, CheckResult};
use vn_sem::scope::ScopeId;
use vn_sem::symbol::SymbolId;
use vn_sem::types::{ObjectTypeMember, Type};

pub(crate) use crate::checker_enrichment::enrich_call_returns;

pub(crate) type MemberTypeCacheEntry = Option<(Type, Option<usize>)>;

pub struct Checker<'r> {
    pub(crate) resolver: &'r dyn vn_sem::resolver::ImportResolver,

    pub(crate) ast_arena: &'r AstArena,
    pub(crate) diagnostics: vn_core::DiagnosticBag,
    pub(crate) source_file: std::sync::Arc<str>,
    pub(crate) current_scope: vn_sem::scope::ScopeId,
    pub(crate) expected_return_type: Option<Type>,
    pub(crate) narrowed_types: FxHashMap<SymbolId, Vec<Type>>,
    pub(crate) member_narrowed: FxHashMap<stmts::reassignment::MemberKey, Vec<Type>>,
    pub(crate) closure_effects: FxHashMap<SymbolId, stmts::closure_effects::ClosureEffects>,
    pub(crate) narrowings_cache:
        FxHashMap<(u32, bool, vn_sem::scope::ScopeId), Vec<(SymbolId, Type)>>,
    pub(crate) child_indices: FxHashMap<ScopeId, usize>,
    pub(crate) infer_cache: FxHashMap<(ExprId, ScopeId, u32), Type>,
    pub(crate) infer_env_rev: u32,
    pub(crate) compat_cache: FxHashMap<(Type, Type, usize), bool>,
    pub(crate) type_node_cache: FxHashMap<(u32, usize), Type>,
    pub(crate) unknown_type_names: rustc_hash::FxHashSet<vn_core::Atom>,
    pub(crate) symbol_type_params_cache: FxHashMap<(Arc<str>, u8), Vec<Arc<str>>>,

    pub(crate) current_class: Option<Arc<str>>,
    pub(crate) extension_self: Option<Type>,
    pub(crate) active_type_params: FxHashSet<Arc<str>>,
    pub(crate) owner_lookup_depth: u32,
    pub(crate) abstract_classes: FxHashSet<Arc<str>>,
    pub(crate) is_assignment_target: bool,
    pub(crate) super_reachable: bool,
    pub(crate) callee_position: bool,
    pub(crate) in_pipeline_rhs: bool,
    pub(crate) pipeline_value_type: Option<Type>,
    pub(crate) member_exists_cache: FxHashMap<(Type, Arc<str>), bool>,
    pub(crate) member_type_cache: FxHashMap<(Type, Arc<str>), MemberTypeCacheEntry>,
    pub(crate) expected_type: Option<Type>,
    pub(crate) expected_pending: bool,
    pub(crate) expected_owner: Option<u32>,
    pub(crate) expected_forwarded: Vec<u32>,

    pub(crate) map_generics_cache: FxHashMap<(Type, Vec<Type>), Type>,
    pub(crate) yielded_types: Option<Vec<Type>>,
    pub(crate) declared_yield: Option<Type>,
    pub(crate) loop_depth: u32,
    pub(crate) switch_depth: u32,
    pub(crate) namespace_bases: rustc_hash::FxHashSet<vn_core::ast::ExprId>,
    pub(crate) loop_labels: Vec<vn_core::Atom>,
    pub(crate) in_function: bool,
    pub(crate) in_async: bool,
    pub(crate) expected_object_members_cache: FxHashMap<Type, Vec<ObjectTypeMember>>,
    pub(crate) warned_deprecated: FxHashSet<(vn_sem::symbol::SymbolId, u32)>,
    pub(crate) pure_scope: Option<vn_sem::scope::ScopeId>,
    pub(crate) enclosing_caps: Option<Vec<String>>,
    pub(crate) ty_table: std::sync::Arc<vn_sem::types::CheckerTyTable>,
}

impl<'r> Checker<'r> {
    pub fn check(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: vn_core::AtomInterner,
        resolver: &'r dyn vn_sem::resolver::ImportResolver,
    ) -> CheckResult {
        Self::check_with(
            program,
            ast_arena,
            interner,
            resolver,
            CheckOptions::compile(),
        )
    }

    pub fn check_with(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: vn_core::AtomInterner,
        resolver: &'r dyn vn_sem::resolver::ImportResolver,
        options: CheckOptions,
    ) -> CheckResult {
        Self::check_internal(program, ast_arena, interner, resolver, options.record_types)
    }

    fn check_internal(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: vn_core::AtomInterner,
        resolver: &'r dyn vn_sem::resolver::ImportResolver,
        record_expr_types: bool,
    ) -> CheckResult {
        let mut profile = CheckProfile::default();
        let (mut bind, source_file) =
            stages::prepare(program, ast_arena, interner, resolver, &mut profile);
        let mut checker = stages::init_checker(
            resolver,
            ast_arena,
            source_file.clone(),
            &bind,
            &mut profile,
        );
        let mut rec = recorder::Recorder::new(record_expr_types);
        stages::run_pass(&mut checker, &mut rec, program, &mut bind, &mut profile);
        stages::assemble(checker, rec, bind, ast_arena, source_file, &mut profile)
    }
}
