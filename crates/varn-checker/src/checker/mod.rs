pub(crate) mod compat;
pub(crate) mod completion;
pub(crate) mod decorator_receiver;
mod foreign_enums;
pub use foreign_enums::ForeignEnum;
mod decl_class;
mod decl_enum;
mod decl_fn;
mod decl_misc;
mod decl_var;
mod decls;
mod definite_assignment;
mod records;
mod refine;
mod scope_records;
mod setup;
mod stmts;
mod type_queries;

use crate::binder::Binder;
use crate::scope::ScopeId;
use crate::symbol::SymbolId;
use crate::types::{ObjectTypeMember, Type};
pub use records::{
    CheckOptions, CheckProfile, CheckResult, Desugarings, ExprInfo, ScopeSpan, TypeEntry,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use varn_core::ast::Program;
use varn_core::ast::{AstArena, ExprId};

pub(crate) use crate::checker_enrichment::enrich_call_returns;

use crate::semantic_info::{CallResolution, MemberResolution};

pub(crate) type MemberTypeCacheEntry = Option<(Type, Option<usize>)>;

pub struct Checker<'r> {
    /// How this checker reaches other modules. Borrowed for the duration of one
    /// check, so the checker owns no module cache and nothing it holds can go
    /// stale behind another thread's invalidation.
    pub(crate) resolver: &'r dyn crate::module_resolver::ImportResolver,
    /// The parsed program's expression/statement nodes (fase1-componente2:
    /// `Expr`/`Stmt` are no longer owned trees). Same field name and same
    /// reused lifetime `'r` as `Binder<'r>::ast_arena` — the checker is a
    /// second, later borrower of the identical arena the binder already
    /// walked, not a distinct mechanism.
    pub(crate) ast_arena: &'r AstArena,
    pub(crate) diagnostics: varn_core::DiagnosticBag,
    pub(crate) source_file: std::sync::Arc<str>,
    pub(crate) current_scope: crate::scope::ScopeId,
    pub(crate) expected_return_type: Option<Type>,
    pub(crate) narrowed_types: FxHashMap<SymbolId, Vec<Type>>,
    pub(crate) narrowings_cache:
        FxHashMap<(u32, bool, crate::scope::ScopeId), Vec<(SymbolId, Type)>>,
    pub(crate) child_indices: FxHashMap<ScopeId, usize>,
    pub(crate) expr_types: FxHashMap<u32, ExprInfo>,
    pub(crate) infer_cache: FxHashMap<(ExprId, ScopeId, u32), Type>,
    pub(crate) infer_env_rev: u32,
    pub(crate) compat_cache: FxHashMap<(Type, Type, usize), bool>,
    pub(crate) type_node_cache: FxHashMap<(u32, usize), Type>,
    pub(crate) symbol_type_params_cache: FxHashMap<(Arc<str>, u8), Vec<Arc<str>>>,
    pub(crate) symbol_types: FxHashMap<SymbolId, Type>,
    pub(crate) expr_table: FxHashMap<varn_core::ast::AstId, TypeEntry>,
    /// Counter behind [`TypeEntry::seq`].
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
    /// Filled by `validate_named_call_arguments`: for a call with named args,
    /// `[param_pos] = Some(arg_idx)` or `None` (omitted, use the default). The
    /// TIR emitter reads it to lay named arguments out positionally.
    pub(crate) call_mappings: FxHashMap<varn_core::ast::AstId, Vec<Option<usize>>>,
    pub(crate) record_expr_types: bool,
    pub(crate) node_scopes: FxHashMap<u32, ScopeId>,
    pub(crate) scope_spans: Vec<ScopeSpan>,
    pub(crate) map_generics_cache: FxHashMap<(Type, Vec<Type>), Type>,
    pub(crate) yielded_types: Option<Vec<Type>>,
    pub warn_implicit_dynamic: bool,
    pub(crate) loop_depth: u32,
    pub(crate) switch_depth: u32,
    pub(crate) in_function: bool,
    pub(crate) expected_object_members_cache: FxHashMap<Type, Vec<ObjectTypeMember>>,
    pub(crate) member_resolutions: FxHashMap<u32, MemberResolution>,
    pub(crate) call_resolutions: FxHashMap<u32, CallResolution>,
    pub(crate) match_gaps: FxHashMap<varn_core::ast::AstId, crate::semantic_info::MatchGap>,
    /// Seeded from `bind.ty_table` and grown as checking synthesizes types
    /// beyond what binding produced (unions from narrowing, instantiated
    /// generics, etc). Same snapshot/publish discipline as `AtomInterner`:
    /// see `ImportResolver::ty_table_snapshot`/`set_ty_table`.
    pub(crate) ty_table: std::sync::Arc<crate::types::CheckerTyTable>,
}

impl<'r> Checker<'r> {
    /// Check `program` for a compile. See [`Checker::check_with`] for tooling.
    ///
    /// `resolver` supplies the modules `program` imports. It is a parameter
    /// rather than ambient state so that a check is a function of its
    /// arguments: two callers with different module graphs cannot interfere,
    /// and nothing the checker consults can be invalidated behind its back.
    pub fn check(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn crate::module_resolver::ImportResolver,
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
        resolver: &'r dyn crate::module_resolver::ImportResolver,
        options: CheckOptions,
    ) -> CheckResult {
        Self::check_internal(
            program,
            ast_arena,
            interner,
            resolver,
            options.record_types,
            options.warn_implicit_dynamic,
        )
    }

    fn check_internal(
        program: &Program,
        ast_arena: &'r AstArena,
        mut interner: varn_core::AtomInterner,
        resolver: &'r dyn crate::module_resolver::ImportResolver,
        record_expr_types: bool,
        warn_implicit_dynamic: bool,
    ) -> CheckResult {
        let mut profile = CheckProfile::default();

        let started = Instant::now();
        let globals_ref = crate::core::loader::module_globals(&program.filename, resolver);
        profile.load_globals = started.elapsed();

        // `core_exports()` may have just bound the core stdlib modules for the
        // first time, minting `Atom`s into the resolver's shared table that
        // `interner` (captured by the caller before this call) never saw. Its
        // `Symbol`s carry those new `Atom`s, so binding against the stale
        // `interner` leaves them unresolvable. This compilation's whole
        // parse/publish discipline (`interner_snapshot`/`set_interner`)
        // already guarantees `interner`'s own entries are a prefix of the
        // resolver's current table, so replacing it here is lossless — every
        // downstream user of `bind.interner`, this file's own atoms included,
        // still resolves correctly.
        //
        // Unconditional, not just `if globals_ref.is_some()`: a core/stdlib
        // module (`is_builtin`, `globals_ref: None`) skips `core_exports()`
        // but can still be checked *after* a sibling core module earlier in
        // the same `compile_stdlib_bundle` loop already grew and published
        // the live table (e.g. a `Generic<T>`-typed symbol whose class-name
        // `Atom` that sibling minted) — this caller's own `interner`, taken
        // before that publish, is exactly as stale either way. Always
        // starting from the live snapshot is never wrong (same prefix
        // guarantee) and is the only branch that actually covers this case.
        interner.absorb(&resolver.interner_snapshot());

        let started = Instant::now();
        let mut bind = match globals_ref {
            Some(globals) => {
                Binder::bind_with_global_refs(program, ast_arena, interner, resolver, &globals)
            }
            None => Binder::bind(program, ast_arena, interner, resolver),
        };
        profile.bind = started.elapsed();

        let started = Instant::now();
        crate::core::merge_core_members(&mut bind, resolver);
        profile.merge_core_members = started.elapsed();

        // `Binder::bind` (and the nested binds it resolves) mints `Atom`s and
        // `CheckerTyId`s after `bind` took its snapshot, so both can be behind
        // the live tables by now. `enrich_call_returns` resolves names and
        // reads types off `bind`, so refresh BOTH here — otherwise
        // `infer_call_type` resolves an atom past the end of `bind.interner`
        // (observed as `index out of bounds: the len is 335 but the index is
        // 335` when checking a core module on the runtime path).
        bind.interner.absorb(&resolver.interner_snapshot());
        let live_ty_table = resolver.ty_table_snapshot();
        if live_ty_table.len() > bind.ty_table.len() {
            std::sync::Arc::make_mut(&mut bind.ty_table).absorb(&live_ty_table);
        }

        let started = Instant::now();
        enrich_call_returns(&mut bind, ast_arena, resolver);
        profile.enrich_call_returns = started.elapsed();

        let source_file: std::sync::Arc<str> = std::sync::Arc::from(bind.source_file.as_ref());

        // `enrich_call_returns` may itself have triggered nested binds
        // (`resolver` calls), so re-adopt live one more time before the
        // checker starts: `absorb` keeps this bind's own id meanings and only
        // learns shapes it lacks (see the pre-enrich block above), and the
        // interner refresh is lossless for the same reason.
        let live_ty_table = resolver.ty_table_snapshot();
        if live_ty_table.len() > bind.ty_table.len() {
            std::sync::Arc::make_mut(&mut bind.ty_table).absorb(&live_ty_table);
        }
        bind.interner.absorb(&resolver.interner_snapshot());
        let started = Instant::now();
        let mut checker = Checker::new(
            resolver,
            ast_arena,
            source_file.clone(),
            bind.global_scope,
            record_expr_types,
            warn_implicit_dynamic,
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

        // Publish types synthesized during checking (narrowed unions,
        // instantiated generics, ...) the same way binding publishes its own
        // growth: `bind.ty_table` carries it onward to emit, and
        // `resolver.set_ty_table` makes it visible to modules that import
        // this one afterward. Same snapshot/publish discipline as
        // `AtomInterner`/`set_interner`.
        bind.ty_table = checker.ty_table.clone();
        resolver.set_ty_table(checker.ty_table.clone());

        // Checking also resolves imports on demand (`module_bind`/
        // `stdlib_bind` inside member lookups, alias expansion, ...), and each
        // of those binds mints atoms into the live table. Emit runs against
        // `bind`, so refresh the atom snapshot once more: emitting a type whose
        // name atom was minted after the last resync would otherwise index out
        // of bounds in `AtomInterner::resolve`.
        bind.interner.absorb(&resolver.interner_snapshot());

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
                                Type(checker.ty_table.get_function(fid).return_type, false)
                                    .is_dynamic()
                            }
                            _ => false,
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
        }
    }
}
