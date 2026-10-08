pub(crate) mod completion;
mod decl_class;
mod decl_enum;
mod decl_fn;
mod decl_misc;
mod decl_var;
mod decls;
pub(crate) mod decorator_receiver;
pub(crate) mod decorator_signature;
mod definite_assignment;
mod foreign_classes;
mod foreign_enums;
mod profile;
pub(crate) mod recorder;
mod refine;
mod scope_records;
mod setup;
mod stages;
mod stmts;
mod type_queries;

use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::Program;
use varn_core::ast::{AstArena, ExprId};
use varn_sem::output::{CheckOptions, CheckProfile, CheckResult};
use varn_sem::scope::ScopeId;
use varn_sem::symbol::SymbolId;
use varn_sem::types::{ObjectTypeMember, Type};

pub(crate) use crate::checker_enrichment::enrich_call_returns;

pub(crate) type MemberTypeCacheEntry = Option<(Type, Option<usize>)>;

pub struct Checker<'r> {
    pub(crate) resolver: &'r dyn varn_sem::resolver::ImportResolver,

    pub(crate) ast_arena: &'r AstArena,
    pub(crate) diagnostics: varn_core::DiagnosticBag,
    pub(crate) source_file: std::sync::Arc<str>,
    pub(crate) current_scope: varn_sem::scope::ScopeId,
    pub(crate) expected_return_type: Option<Type>,
    pub(crate) narrowed_types: FxHashMap<SymbolId, Vec<Type>>,
    pub(crate) narrowings_cache:
        FxHashMap<(u32, bool, varn_sem::scope::ScopeId), Vec<(SymbolId, Type)>>,
    pub(crate) child_indices: FxHashMap<ScopeId, usize>,
    pub(crate) infer_cache: FxHashMap<(ExprId, ScopeId, u32), Type>,
    pub(crate) infer_env_rev: u32,
    pub(crate) compat_cache: FxHashMap<(Type, Type, usize), bool>,
    pub(crate) type_node_cache: FxHashMap<(u32, usize), Type>,
    pub(crate) symbol_type_params_cache: FxHashMap<(Arc<str>, u8), Vec<Arc<str>>>,

    pub(crate) current_class: Option<Arc<str>>,
    pub(crate) active_type_params: FxHashSet<Arc<str>>,
    pub(crate) abstract_classes: FxHashSet<Arc<str>>,
    pub(crate) is_assignment_target: bool,
    pub(crate) in_pipeline_rhs: bool,
    pub(crate) pipeline_value_type: Option<Type>,
    pub(crate) member_exists_cache: FxHashMap<(Type, Arc<str>), bool>,
    pub(crate) member_type_cache: FxHashMap<(Type, Arc<str>), MemberTypeCacheEntry>,
    pub(crate) expected_type: Option<Type>,

    pub(crate) map_generics_cache: FxHashMap<(Type, Vec<Type>), Type>,
    pub(crate) yielded_types: Option<Vec<Type>>,
    pub(crate) loop_depth: u32,
    pub(crate) switch_depth: u32,
    pub(crate) in_function: bool,
    pub(crate) in_async: bool,
    pub(crate) expected_object_members_cache: FxHashMap<Type, Vec<ObjectTypeMember>>,
    pub(crate) warned_deprecated: FxHashSet<(varn_sem::symbol::SymbolId, u32)>,
    pub(crate) pure_scope: Option<varn_sem::scope::ScopeId>,
    pub(crate) enclosing_caps: Option<Vec<String>>,
    pub(crate) ty_table: std::sync::Arc<varn_sem::types::CheckerTyTable>,
}

impl<'r> Checker<'r> {
    pub fn check(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn varn_sem::resolver::ImportResolver,
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
        interner: varn_core::AtomInterner,
        resolver: &'r dyn varn_sem::resolver::ImportResolver,
        options: CheckOptions,
    ) -> CheckResult {
        Self::check_internal(program, ast_arena, interner, resolver, options.record_types)
    }

    fn check_internal(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn varn_sem::resolver::ImportResolver,
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
