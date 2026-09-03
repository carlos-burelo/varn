pub(crate) mod compat;
mod decls;
mod refine;
mod stmts;
use crate::binder::{BindResult, BindView, Binder};
use crate::scope::ScopeId;
use crate::symbol::SymbolId;
use crate::types::{ObjectTypeMember, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use varn_core::ast::Expr;
use varn_core::ast::Program;
use varn_core::Diagnostic;

pub(crate) use crate::checker_annotations::collect_type_annotations;
pub(crate) use crate::checker_enrichment::enrich_call_returns;

use crate::semantic_info::{CallResolution, MemberResolution};

pub(crate) type MemberTypeCacheEntry = Option<(Type, Option<usize>)>;

#[derive(Clone, Debug)]
pub struct ExprInfo {
    pub ty: Type,
    pub symbol_id: Option<SymbolId>,
}

/// What the checker decided about one expression.
///
/// Keyed by [`varn_core::ast::AstId`] in [`CheckResult::expr_table`], which is
/// the single record of the checker's answers. The positional map the editor
/// queries (`CheckResult::expr_types`) is PROJECTED from this at the end of a
/// check — it is an index, not a second opinion.
#[derive(Clone, Debug)]
pub struct TypeEntry {
    /// The type the program was CHECKED against. Every diagnostic is reported
    /// against this and nothing else.
    pub ty: Type,
    /// The strongest fact PROVED about this value, when it is stronger than
    /// [`Self::ty`]. Consumed only by codegen; `None` means nothing is known
    /// beyond the checked type.
    ///
    /// This lane exists because a prover and a type checker want opposite
    /// failure modes. `binder::array_evolve` states it as design rule 4: a
    /// proof that turns out weaker than reality must cost an OPTIMISATION,
    /// never a spurious error in a valid program — `let a = []; a.push(1);
    /// return a[0]` has to keep compiling inside a `: str` function even
    /// though the element is provably `int`. Keeping the proof out of `ty`
    /// isolates the type system from the analyser's reach.
    pub refined: Option<Type>,
    pub start: u32,
    pub end: u32,
    pub seq: u32,
    pub symbol_id: Option<SymbolId>,
}

impl std::fmt::Display for ExprInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.ty)
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScopeSpan {
    pub start: u32,
    pub end: u32,
    pub scope: ScopeId,
}

pub struct CheckResult {
    pub bind: BindResult,
    pub diagnostics: varn_core::DiagnosticBag,
    /// Positional index over [`Self::expr_table`], for editors asking "what is
    /// at this cursor". Empty unless [`CheckOptions::record_types`] was set.
    ///
    /// Derived, never authored: it used to be written inline during checking,
    /// alongside a separate id-keyed map, which is how the two came to hold
    /// different things.
    pub expr_types: FxHashMap<u32, ExprInfo>,
    pub flattened_members: FxHashMap<Rc<str>, Vec<crate::types::ClassMemberInfo>>,
    pub type_annotations: varn_core::TypeAnnotations,
    pub profile: CheckProfile,
    pub extension_calls: FxHashMap<u32, Rc<str>>,
    pub extension_members: FxHashMap<u32, Rc<str>>,
    pub extension_set_members: FxHashMap<u32, Rc<str>>,
    pub node_scopes: FxHashMap<u32, crate::scope::ScopeId>,
    pub scope_spans: Vec<ScopeSpan>,
    pub symbol_types: FxHashMap<SymbolId, crate::types::Type>,
    pub member_resolutions: FxHashMap<u32, MemberResolution>,
    pub call_resolutions: FxHashMap<u32, CallResolution>,
    /// **The** record of what the checker decided, keyed by `Expr::id()`.
    ///
    /// Everything else that carries an expression's type is derived from this:
    /// [`Self::expr_types`] is its positional projection, and the codegen
    /// annotations are built by reading it. Written on every check, tooling or
    /// not, because a compile needs these types too.
    pub expr_table: FxHashMap<varn_core::ast::AstId, TypeEntry>,
}

impl CheckResult {
    pub fn scope_at_offset(&self, cursor_offset: u32) -> crate::scope::ScopeId {
        let mut best_scope = self.bind.global_scope;
        let mut best_span_len = u32::MAX;
        for span in &self.scope_spans {
            if cursor_offset >= span.start && cursor_offset <= span.end {
                let span_len = span.end.saturating_sub(span.start);
                if span_len < best_span_len {
                    best_span_len = span_len;
                    best_scope = span.scope;
                }
            }
        }
        best_scope
    }

    pub fn resolve_at(
        &self,
        name: &str,
        cursor_offset: u32,
    ) -> Option<(SymbolId, crate::types::Type)> {
        let scope_id = self.scope_at_offset(cursor_offset);
        let scope = self.bind.scopes.get(scope_id);
        let sym_id = scope.resolve(name, &self.bind.scopes)?;
        let ty = self
            .symbol_types
            .get(&sym_id)
            .cloned()
            .or_else(|| self.bind.arena.get(sym_id).ty.clone())
            .unwrap_or(crate::types::Type::Dynamic);
        Some((sym_id, ty))
    }
}

#[derive(Clone, Debug, Default)]
pub struct CheckProfile {
    pub load_globals: Duration,
    pub bind: Duration,
    pub merge_core_members: Duration,
    pub enrich_call_returns: Duration,
    /// Checker struct construction + abstract_classes scan.
    pub init: Duration,
    pub check_stmts: Duration,
    pub collect_annotations: Duration,
    pub finalize: Duration,
    /// symbol_types clone + CheckResult construction.
    pub cleanup: Duration,
}

pub struct Checker<'r> {
    /// How this checker reaches other modules. Borrowed for the duration of one
    /// check, so the checker owns no module cache and nothing it holds can go
    /// stale behind another thread's invalidation.
    pub(crate) resolver: &'r dyn crate::module_resolver::ImportResolver,
    pub(crate) diagnostics: varn_core::DiagnosticBag,
    pub(crate) source_file: std::rc::Rc<str>,
    pub(crate) current_scope: crate::scope::ScopeId,
    pub(crate) expected_return_type: Option<Type>,
    pub(crate) narrowed_types: FxHashMap<SymbolId, Vec<Type>>,
    pub(crate) narrowings_cache:
        FxHashMap<(u32, bool, crate::scope::ScopeId), Vec<(SymbolId, Type)>>,
    pub(crate) child_indices: FxHashMap<ScopeId, usize>,
    pub(crate) expr_types: FxHashMap<u32, ExprInfo>,
    pub(crate) infer_cache: FxHashMap<(u32, ScopeId, u32), Type>,
    pub(crate) infer_env_rev: u32,
    pub(crate) compat_cache: FxHashMap<(Type, Type, usize), bool>,
    pub(crate) type_node_cache: FxHashMap<(u32, usize), Type>,
    pub(crate) symbol_type_params_cache: FxHashMap<(Rc<str>, u8), Vec<Rc<str>>>,
    pub(crate) symbol_types: FxHashMap<SymbolId, Type>,
    pub(crate) expr_table: FxHashMap<varn_core::ast::AstId, TypeEntry>,
    /// Counter behind [`TypeEntry::seq`].
    pub(crate) expr_seq: u32,
    pub(crate) current_class: Option<Rc<str>>,
    pub(crate) active_type_params: FxHashSet<Rc<str>>,
    pub(crate) abstract_classes: FxHashSet<Rc<str>>,
    pub(crate) is_assignment_target: bool,
    pub(crate) in_pipeline_rhs: bool,
    pub(crate) pipeline_value_type: Option<Type>,
    pub(crate) extension_calls: FxHashMap<u32, Rc<str>>,
    pub(crate) extension_members: FxHashMap<u32, Rc<str>>,
    pub(crate) extension_set_members: FxHashMap<u32, Rc<str>>,
    pub(crate) member_exists_cache: FxHashMap<(Type, Rc<str>), bool>,
    pub(crate) member_type_cache: FxHashMap<(Type, Rc<str>), MemberTypeCacheEntry>,
    pub(crate) expected_type: Option<Type>,
    pub(crate) call_mappings: FxHashMap<varn_core::ast::AstId, Vec<Option<usize>>>,
    pub(crate) reassigned_names: rustc_hash::FxHashSet<Rc<str>>,
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
}

/// What a caller wants from a check, beyond the diagnostics.
///
/// One options struct instead of four constructors. `check_for_lsp` and
/// `check_with_profile` used to be byte-for-byte the same call under two
/// names, so a reader could not tell whether they were meant to differ — and
/// `vn bench` picked `check_with_profile`, which meant the timings it reported
/// were the TOOLING configuration, not the one that compiles. Profiling is not
/// an option at all: `CheckResult::profile` is always filled.
#[derive(Clone, Copy, Debug, Default)]
pub struct CheckOptions {
    /// Build the per-expression type table (`CheckResult::expr_types`).
    ///
    /// Tooling needs it to answer "what is at this cursor". A compile does
    /// not, and building it costs an insert per expression. It must never
    /// change what any type IS — see the comment in `Checker::infer_type`.
    pub record_types: bool,
    /// Warn wherever a type fell back to `dynamic` rather than being inferred.
    pub warn_implicit_dynamic: bool,
}

impl CheckOptions {
    /// What a compile wants: diagnostics and annotations, no type table.
    pub fn compile() -> Self {
        Self::default()
    }

    /// What an editor wants: the type table as well.
    pub fn tooling() -> Self {
        Self {
            record_types: true,
            ..Self::default()
        }
    }

    pub fn strict(mut self) -> Self {
        self.warn_implicit_dynamic = true;
        self
    }
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
        resolver: &'r dyn crate::module_resolver::ImportResolver,
    ) -> CheckResult {
        Self::check_with(program, resolver, CheckOptions::compile())
    }

    pub fn check_with(
        program: &Program,
        resolver: &'r dyn crate::module_resolver::ImportResolver,
        options: CheckOptions,
    ) -> CheckResult {
        Self::check_internal(
            program,
            resolver,
            options.record_types,
            options.warn_implicit_dynamic,
        )
    }

    fn check_internal(
        program: &Program,
        resolver: &'r dyn crate::module_resolver::ImportResolver,
        record_expr_types: bool,
        warn_implicit_dynamic: bool,
    ) -> CheckResult {
        let mut profile = CheckProfile::default();

        let is_builtin = crate::core::is_core_file(&program.filename);
        let globals_ref = if !is_builtin {
            let started = Instant::now();
            let globals = resolver.core_exports();
            profile.load_globals = started.elapsed();
            Some(globals)
        } else {
            None
        };

        let started = Instant::now();
        let mut bind = match globals_ref {
            Some(globals) => Binder::bind_with_global_refs(program, resolver, &globals),
            None => Binder::bind(program, resolver),
        };
        profile.bind = started.elapsed();

        let started = Instant::now();
        crate::core::merge_core_members(&mut bind, resolver);
        profile.merge_core_members = started.elapsed();

        let started = Instant::now();
        enrich_call_returns(&mut bind, resolver);
        profile.enrich_call_returns = started.elapsed();

        let source_file: std::rc::Rc<str> = std::rc::Rc::from(bind.source_file.as_ref());

        let started = Instant::now();
        let mut checker = Checker {
            resolver,
            current_scope: bind.global_scope,
            diagnostics: varn_core::DiagnosticBag::new(),
            source_file: source_file.clone(),
            expected_return_type: None,
            narrowed_types: FxHashMap::default(),
            narrowings_cache: FxHashMap::default(),
            child_indices: FxHashMap::with_capacity_and_hasher(64, Default::default()),
            expr_types: if record_expr_types {
                FxHashMap::with_capacity_and_hasher(2048, Default::default())
            } else {
                FxHashMap::default()
            },
            infer_cache: FxHashMap::with_capacity_and_hasher(4096, Default::default()),
            infer_env_rev: 0,
            compat_cache: FxHashMap::with_capacity_and_hasher(4096, Default::default()),
            type_node_cache: FxHashMap::with_capacity_and_hasher(1024, Default::default()),
            symbol_type_params_cache: FxHashMap::with_capacity_and_hasher(256, Default::default()),
            symbol_types: FxHashMap::default(),
            expr_table: FxHashMap::with_capacity_and_hasher(2048, Default::default()),
            expr_seq: 0,
            current_class: None,
            active_type_params: FxHashSet::default(),
            abstract_classes: FxHashSet::default(),
            is_assignment_target: false,
            in_pipeline_rhs: false,
            pipeline_value_type: None,
            extension_calls: FxHashMap::default(),
            extension_members: FxHashMap::default(),
            extension_set_members: FxHashMap::default(),
            member_exists_cache: FxHashMap::with_capacity_and_hasher(256, Default::default()),
            member_type_cache: FxHashMap::with_capacity_and_hasher(1024, Default::default()),
            warn_implicit_dynamic,
            expected_type: None,
            call_mappings: FxHashMap::default(),
            reassigned_names: rustc_hash::FxHashSet::default(),
            record_expr_types,
            node_scopes: FxHashMap::default(),
            scope_spans: Vec::new(),
            map_generics_cache: FxHashMap::with_capacity_and_hasher(512, Default::default()),
            yielded_types: None,
            loop_depth: 0,
            switch_depth: 0,
            in_function: false,
            expected_object_members_cache: FxHashMap::with_capacity_and_hasher(
                512,
                Default::default(),
            ),
            member_resolutions: if record_expr_types {
                FxHashMap::with_capacity_and_hasher(512, Default::default())
            } else {
                FxHashMap::default()
            },
            call_resolutions: if record_expr_types {
                FxHashMap::with_capacity_and_hasher(512, Default::default())
            } else {
                FxHashMap::default()
            },
        };

        for (name, class_info) in &bind.type_members.classes {
            if class_info.is_abstract {
                checker.abstract_classes.insert(name.clone());
            }
        }
        profile.init = started.elapsed();

        let started = Instant::now();
        checker.check_stmts(&program.body, &bind);
        profile.check_stmts = started.elapsed();

        // Positional projection of `expr_table`, for editors. Built here, from
        // the one table, rather than written alongside it during the check:
        // that is what keeps "what the checker decided" and "what the editor
        // shows" from being two different answers.
        if record_expr_types {
            let mut entries: Vec<(u32, u32, u32, ExprInfo)> = checker
                .expr_table
                .values()
                .map(|e| {
                    let span_len = e.end.saturating_sub(e.start);
                    let info = ExprInfo {
                        ty: e.ty.clone(),
                        symbol_id: e.symbol_id,
                    };
                    (e.start, span_len, e.seq, info)
                })
                .collect();
            // Sort by span length DESCENDING, so smaller/innermost (more specific) spans overwrite larger parent spans at the same start offset
            entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
            for (start, _len, _seq, info) in entries {
                checker.expr_types.insert(start, info);
            }

            // Declarations the binder created but no expression visits —
            // parameters, methods, getters/setters, fields — have no entry in
            // `expr_table` at all, so their own name token would resolve to
            // nothing. `or_insert` keeps any finer entry the projection above
            // already produced.
            for (id, sym) in bind.arena.all().iter().enumerate() {
                if sym.origin_module.is_none() && sym.offset != 0 {
                    checker
                        .expr_types
                        .entry(sym.offset)
                        .or_insert_with(|| ExprInfo {
                            ty: sym.ty.clone().unwrap_or(Type::Dynamic),
                            symbol_id: Some(id),
                        });
                }
            }
        }

        let mut final_diagnostics = std::mem::take(&mut bind.diagnostics);
        final_diagnostics.extend(checker.diagnostics);

        let started = Instant::now();
        let expr_table = std::mem::take(&mut checker.expr_table);
        let mut annotations = collect_type_annotations(program, &bind, resolver, &expr_table);

        for (k, v) in checker.call_mappings {
            annotations.record_call_mapping(varn_core::AnnKey::expr(k), v);
        }
        for name in &checker.reassigned_names {
            annotations.record_reassigned_name(name);
        }
        profile.collect_annotations = started.elapsed();
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
                        || match &t.0 {
                            varn_core::TypeKind::Fn(ft) => ft.return_type.is_dynamic(),
                            _ => false,
                        }
                }
            };

            if !sym.has_explicit_type && current_is_weak {
                sym.ty = Some(ty.clone());
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
            type_annotations: annotations,
            profile,
            extension_calls: checker.extension_calls,
            extension_members: checker.extension_members,
            extension_set_members: checker.extension_set_members,
            node_scopes,
            scope_spans,
            symbol_types,
            member_resolutions: checker.member_resolutions,
            call_resolutions: checker.call_resolutions,
            expr_table,
        }
    }

    #[inline]
    pub(crate) fn emit(&mut self, diag: Diagnostic) {
        let diag = if diag.file.is_empty() {
            diag.with_file(self.source_file.clone())
        } else {
            diag
        };
        self.diagnostics.push(diag);
    }

    pub(crate) fn record_scope(&mut self, offset: u32) {
        if self.record_expr_types {
            self.node_scopes.insert(offset, self.current_scope);
        }
    }

    pub(crate) fn record_scope_span(&mut self, start: u32, end: u32, scope: ScopeId) {
        if self.record_expr_types {
            self.scope_spans.push(ScopeSpan { start, end, scope });
            self.node_scopes.insert(start, scope);
        }
    }

    pub(crate) fn with_next_child_scope<R>(
        &mut self,
        bind: &BindResult,
        offset: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved_scope = self.current_scope;
        if let Some(child) = self.next_child_scope(bind) {
            self.current_scope = child;
            self.record_scope(offset);
        }
        let res = f(self);
        self.current_scope = saved_scope;
        res
    }

    pub(crate) fn with_next_child_scope_span<R>(
        &mut self,
        bind: &BindResult,
        start: u32,
        end: u32,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved_scope = self.current_scope;
        if let Some(child) = self.next_child_scope(bind) {
            self.current_scope = child;
            self.record_scope_span(start, end, child);
        }
        let res = f(self);
        self.current_scope = saved_scope;
        res
    }

    pub(crate) fn with_expected<R>(
        &mut self,
        ty: Option<Type>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = self.expected_type.take();
        self.expected_type = ty;
        let result = f(self);
        self.expected_type = prev;
        result
    }

    /// Run `f` with the traversal state that says "we are inside a function
    /// body": `return` is legal, and `break`/`continue` cannot reach a loop or
    /// switch outside the body.
    ///
    /// Object-literal methods, getters and setters used to skip this, so every
    /// `return` inside one was reported as WR4007, "a 'return' statement can
    /// only be used within a function body".
    pub(crate) fn in_function_body<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved_in_function = self.in_function;
        let saved_loop_depth = self.loop_depth;
        let saved_switch_depth = self.switch_depth;
        self.in_function = true;
        self.loop_depth = 0;
        self.switch_depth = 0;

        let result = f(self);

        self.in_function = saved_in_function;
        self.loop_depth = saved_loop_depth;
        self.switch_depth = saved_switch_depth;
        result
    }

    pub(crate) fn infer_type(&mut self, expr: &Expr, bind: &BindResult) -> Type {
        let key = (expr.id(), self.current_scope, self.infer_env_rev);
        if let Some(ty) = self.infer_cache.get(&key) {
            return ty.clone();
        }

        // Inference does NOT depend on whether the caller wants a type table.
        //
        // This used to switch `current_scope` to `node_scopes[expr.id()]` when
        // `record_expr_types` was on, which made the tooling path capable of
        // inferring a different type than the compile path for the same
        // expression. It was also reading a map written with a DIFFERENT key:
        // `record_scope` inserts by byte offset at 18 sites, so a lookup by AST
        // id either missed or — worse — hit an unrelated node whose offset
        // happened to equal this node's id.
        //
        // Recording a type is a map insert. It is not allowed to change what
        // the type is.
        let ty = self.infer_type_internal(expr, bind);

        self.infer_cache.insert(key, ty.clone());
        ty
    }

    pub(crate) fn next_child_scope(&mut self, bind: &BindResult) -> Option<ScopeId> {
        let children = &bind.scopes.get(self.current_scope).children;
        let idx = self.child_indices.entry(self.current_scope).or_insert(0);
        if *idx < children.len() {
            let child_id = children[*idx];
            *idx += 1;
            Some(child_id)
        } else {
            None
        }
    }

    pub(crate) fn types_compatible_cached(
        &mut self,
        declared: &Type,
        inferred: &Type,
        bind: Option<&BindResult>,
    ) -> bool {
        // The view is built here, at the one funnel every caller passes
        // through, so none of the 22 call sites has to carry a resolver.
        let resolver = self.resolver;
        let view = bind.map(|b| BindView::new(b, resolver));
        compat::types_compatible_with_cache(
            declared,
            inferred,
            view.as_ref(),
            &mut self.compat_cache,
        )
    }

    pub(crate) fn mark_infer_env_dirty(&mut self) {
        self.infer_env_rev = self.infer_env_rev.wrapping_add(1);
        if self.infer_cache.len() > 16_384 {
            self.infer_cache.clear();
        }
    }

    pub(crate) fn resolve_type_node_cached(
        &mut self,
        node: &varn_core::ast::TypeNode,
        bind: &BindResult,
    ) -> Type {
        let key = (node.range.start.offset, bind as *const BindResult as usize);
        if let Some(cached) = self.type_node_cache.get(&key) {
            return cached.clone();
        }
        let view = crate::binder::BindView::new(bind, self.resolver);
        let resolved = crate::binder::resolve_type_node(node, Some(&view));
        self.type_node_cache.insert(key, resolved.clone());
        resolved
    }

    pub(crate) fn symbol_type_params(
        &mut self,
        name: &str,
        kind: crate::symbol::SymbolKind,
        bind: &BindResult,
    ) -> Vec<Rc<str>> {
        let key = (Rc::from(name), symbol_kind_cache_key(kind));
        if let Some(cached) = self.symbol_type_params_cache.get(&key) {
            return cached.clone();
        }

        let resolved = if let Some(sid) = bind
            .scopes
            .get(bind.global_scope)
            .resolve(name, &bind.scopes)
        {
            let sym = bind.arena.get(sid);
            if sym.kind == kind {
                sym.type_params.clone()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        self.symbol_type_params_cache.insert(key, resolved.clone());
        resolved
    }

    pub(crate) fn symbol_type_params_any(&mut self, name: &str, bind: &BindResult) -> Vec<Rc<str>> {
        let key = (Rc::from(name), 255);
        if let Some(cached) = self.symbol_type_params_cache.get(&key) {
            return cached.clone();
        }

        let resolved = if let Some(sid) = bind
            .scopes
            .get(bind.global_scope)
            .resolve(name, &bind.scopes)
        {
            bind.arena.get(sid).type_params.clone()
        } else {
            bind.core
                .as_ref()
                .and_then(|b| b.class_type_params.get(name))
                .cloned()
                .unwrap_or_default()
        };

        self.symbol_type_params_cache.insert(key, resolved.clone());
        resolved
    }
}

fn symbol_kind_cache_key(kind: crate::symbol::SymbolKind) -> u8 {
    match kind {
        crate::symbol::SymbolKind::Var => 0,
        crate::symbol::SymbolKind::Let => 1,
        crate::symbol::SymbolKind::Const => 2,
        crate::symbol::SymbolKind::Function => 3,
        crate::symbol::SymbolKind::Class => 4,
        crate::symbol::SymbolKind::Interface => 5,
        crate::symbol::SymbolKind::TypeAlias => 6,
        crate::symbol::SymbolKind::Enum => 7,
        crate::symbol::SymbolKind::Parameter => 8,
        crate::symbol::SymbolKind::Property => 9,
        crate::symbol::SymbolKind::Method => 10,
        crate::symbol::SymbolKind::TypeParameter => 11,
        crate::symbol::SymbolKind::Namespace => 12,
        crate::symbol::SymbolKind::Struct => 13,
        crate::symbol::SymbolKind::Extension => 14,
        crate::symbol::SymbolKind::EnumMember => 15,
    }
}
