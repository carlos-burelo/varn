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
mod refine;
mod scope_records;
mod setup;
mod stmts;
mod type_queries;

use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use varn_binder::Binder;
use varn_core::ast::Program;
use varn_core::ast::{AstArena, ExprId};
use varn_sem::output::{
    CheckOptions, CheckProfile, CheckResult, Desugarings, ExprInfo, ScopeSpan, TypeEntry,
};
use varn_sem::scope::ScopeId;
use varn_sem::symbol::SymbolId;
use varn_sem::types::{ObjectTypeMember, Type};

pub(crate) use crate::checker_enrichment::enrich_call_returns;

use varn_sem::semantic_info::{CallResolution, MemberResolution};

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
    pub(crate) expr_types: FxHashMap<u32, ExprInfo>,
    pub(crate) infer_cache: FxHashMap<(ExprId, ScopeId, u32), Type>,
    pub(crate) infer_env_rev: u32,
    pub(crate) compat_cache: FxHashMap<(Type, Type, usize), bool>,
    pub(crate) type_node_cache: FxHashMap<(u32, usize), Type>,
    pub(crate) symbol_type_params_cache: FxHashMap<(Arc<str>, u8), Vec<Arc<str>>>,
    pub(crate) symbol_types: FxHashMap<SymbolId, Type>,
    pub(crate) expr_table: FxHashMap<varn_core::ast::AstId, TypeEntry>,

    pub(crate) expr_seq: u32,
    pub(crate) current_class: Option<Arc<str>>,
    pub(crate) active_type_params: FxHashSet<Arc<str>>,
    pub(crate) abstract_classes: FxHashSet<Arc<str>>,
    pub(crate) is_assignment_target: bool,
    pub(crate) in_pipeline_rhs: bool,
    pub(crate) pipeline_value_type: Option<Type>,
    pub(crate) desugar: Desugarings,
    pub(crate) member_exists_cache: FxHashMap<(Type, Arc<str>), bool>,
    pub(crate) member_type_cache: FxHashMap<(Type, Arc<str>), MemberTypeCacheEntry>,
    pub(crate) expected_type: Option<Type>,

    pub(crate) call_mappings: FxHashMap<varn_core::ast::AstId, Vec<Option<usize>>>,
    pub(crate) record_expr_types: bool,
    pub(crate) node_scopes: FxHashMap<u32, ScopeId>,
    pub(crate) scope_spans: Vec<ScopeSpan>,
    pub(crate) map_generics_cache: FxHashMap<(Type, Vec<Type>), Type>,
    pub(crate) yielded_types: Option<Vec<Type>>,
    pub(crate) loop_depth: u32,
    pub(crate) switch_depth: u32,
    pub(crate) in_function: bool,
    pub(crate) in_async: bool,
    pub(crate) expected_object_members_cache: FxHashMap<Type, Vec<ObjectTypeMember>>,
    pub(crate) member_resolutions: FxHashMap<u32, MemberResolution>,
    pub(crate) call_resolutions: FxHashMap<u32, CallResolution>,
    pub(crate) match_gaps: FxHashMap<varn_core::ast::AstId, varn_sem::semantic_info::MatchGap>,
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

        let started = Instant::now();
        let globals_ref = varn_binder::core::loader::module_globals(&program.filename, resolver);
        profile.load_globals = started.elapsed();

        let started = Instant::now();
        let mut bind = match globals_ref {
            Some(globals) => {
                Binder::bind_with_global_refs(program, ast_arena, interner, resolver, &globals)
            }
            None => Binder::bind(program, ast_arena, interner, resolver),
        };
        profile.bind = started.elapsed();

        let started = Instant::now();
        varn_binder::core::loader::merge_core_members(&mut bind, resolver);
        profile.merge_core_members = started.elapsed();

        let started = Instant::now();
        enrich_call_returns(&mut bind, ast_arena, resolver);
        profile.enrich_call_returns = started.elapsed();

        let source_file: std::sync::Arc<str> = std::sync::Arc::from(bind.source_file.as_ref());

        let started = Instant::now();
        let mut checker = Checker::new(
            resolver,
            ast_arena,
            source_file.clone(),
            bind.global_scope,
            record_expr_types,
            bind.ty_table.clone(),
        );

        for (name, class_info) in &bind.type_members.classes {
            if class_info.is_abstract {
                checker.abstract_classes.insert(name.clone());
            }
        }
        profile.init = started.elapsed();

        let started = Instant::now();
        checker.check_stmts(&program.body, &bind);
        checker.check_definite_assignment(program, &bind);
        profile.check_stmts = started.elapsed();

        if record_expr_types {
            checker.project_expr_types(&bind);
        }

        checker.desugar.foreign_inherited_fields = checker.collect_foreign_inherited_fields(&bind);
        bind.ty_table = checker.ty_table.clone();
        bind.interner.absorb(checker.ty_table.names());

        checker.desugar.foreign_enums = checker
            .collect_foreign_enums(&bind, checker.expr_table.values().map(|entry| &entry.ty));

        let mut final_diagnostics = std::mem::take(&mut bind.diagnostics);
        final_diagnostics.extend(checker.diagnostics);
        for (kept, rejected) in bind.interner.collisions() {
            final_diagnostics.emit(varn_core::Diagnostic::error(
                varn_core::ErrorCode::CompilationInternalError,
                format!("name hash collision: '{kept}' and '{rejected}' share an Atom"),
            ));
        }

        let expr_table = std::mem::take(&mut checker.expr_table);
        let mut test_targets = Vec::new();
        for sym in bind.arena.all() {
            if sym.is_test && sym.origin_module.is_none() {
                test_targets.push(varn_sem::output::TestTarget {
                    name: Arc::from(bind.interner.resolve(sym.name)),
                    file: source_file.clone(),
                    is_async: sym.is_async,
                });
            }
        }
        if !final_diagnostics.has_errors() {
            if let Some(&id) = expr_table
                .iter()
                .filter(|(_, entry)| entry.ty.is_error())
                .map(|(id, _)| id)
                .min()
            {
                let mut diag = varn_core::Diagnostic::error(
                    varn_core::ErrorCode::CompilationInternalError,
                    "type resolution failed here without reporting why",
                );
                if let Some(node) = ast_arena.exprs().nth(id as usize) {
                    diag = diag.with_range(node.range);
                }
                final_diagnostics.emit(diag);
            }
        }
        profile.collect_annotations = Duration::ZERO;
        let flattened = std::mem::take(&mut bind.type_members.flattened);

        let started = Instant::now();
        for (sid, ty) in &checker.symbol_types {
            let sym = bind.arena.get_mut(*sid);

            if sym.origin_module.is_some() {
                continue;
            }

            let current_is_weak = match &sym.ty {
                None => true,
                Some(t) => {
                    t.is_dynamic()
                        || match checker.ty_table.get(t.0) {
                            varn_core::TypeKind::Fn(fid) => {
                                Type::resolved(checker.ty_table.get_function(fid).return_type)
                                    .is_dynamic()
                            }
                            varn_core::TypeKind::Primitive(_)
                            | varn_core::TypeKind::Builtin(_)
                            | varn_core::TypeKind::Literal(_)
                            | varn_core::TypeKind::This
                            | varn_core::TypeKind::Array(_)
                            | varn_core::TypeKind::Union(_)
                            | varn_core::TypeKind::Intersection(_)
                            | varn_core::TypeKind::Tuple(_)
                            | varn_core::TypeKind::Named(..)
                            | varn_core::TypeKind::Generic(..)
                            | varn_core::TypeKind::TemplateLiteral(_)
                            | varn_core::TypeKind::Object(_)
                            | varn_core::TypeKind::Typeof(_)
                            | varn_core::TypeKind::KeyOf(_)
                            | varn_core::TypeKind::IndexedAccess { .. }
                            | varn_core::TypeKind::Mapped { .. }
                            | varn_core::TypeKind::Conditional { .. }
                            | varn_core::TypeKind::Infer(_)
                            | varn_core::TypeKind::EnumVariant { .. }
                            | varn_core::TypeKind::TypePredicate { .. } => false,
                        }
                }
            };

            if !sym.has_explicit_type && current_is_weak {
                sym.ty = Some(*ty);
            }
        }
        profile.finalize = started.elapsed();

        let started = Instant::now();
        let symbol_types = checker.symbol_types.clone();
        let node_scopes = if record_expr_types {
            checker.node_scopes.clone()
        } else {
            FxHashMap::default()
        };
        let scope_spans = if record_expr_types {
            checker.scope_spans
        } else {
            Vec::new()
        };

        profile.cleanup = started.elapsed();

        CheckResult {
            bind,
            diagnostics: final_diagnostics,
            expr_types: checker.expr_types,
            flattened_members: flattened,
            profile,
            node_scopes,
            scope_spans,
            symbol_types,
            member_resolutions: checker.member_resolutions,
            call_resolutions: checker.call_resolutions,
            match_gaps: checker.match_gaps,
            expr_table,
            call_mappings: checker.call_mappings,
            desugar: checker.desugar,
            test_targets,
        }
    }
}
