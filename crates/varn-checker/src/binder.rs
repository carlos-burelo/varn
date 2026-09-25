use crate::scope::{CheckerScope, ScopeArena, ScopeId, ScopeKind};
use crate::symbol::{Symbol, SymbolArena, SymbolKind};
use crate::types::Type;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, ExprKind, ForInit, Program, StmtId, StmtKind, VarDeclarator};

mod array_evolve;
mod class;
mod decl_values;
mod declared_types;
mod decls;
mod definite_field_assignment;
mod diagnostics;
mod imports;
mod inference_utils;
mod interface;
pub(crate) mod type_inference;
mod type_resolution;
mod types;

use crate::module_resolver::ImportResolver;
pub use crate::types::{ClassMemberInfo, ClassMemberKind, TypeContext};
pub use inference_utils::build_fn_type;
pub use type_inference::{infer_expr_type, pattern_lead_name, widen_literal};
pub use type_resolution::{resolve_primitive, resolve_type_node};
pub use types::{BindResult, BindView, Extensions, PendingEnrich, TypeMembers};
use varn_core::ast::{Pattern, TypeNode, VarKind};

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
}

impl TypeContext for Binder<'_> {
    fn resolver(&self) -> Option<&dyn crate::module_resolver::ImportResolver> {
        Some(self.resolver)
    }

    fn interner(&self) -> Option<&varn_core::AtomInterner> {
        Some(&self.interner)
    }

    fn ty_table(&self) -> Option<&crate::types::CheckerTyTable> {
        Some(&self.ty_table)
    }

    fn ast_arena(&self) -> Option<&varn_core::ast::AstArena> {
        Some(self.ast_arena)
    }

    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = self
                    .resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))
                {
                    return rb.type_members.interfaces.get(name).cloned();
                }
            }
        }
        self.type_members.interfaces.get(name).cloned()
    }

    fn get_class_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = self
                    .resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))
                {
                    return rb.type_members.classes.get(name).map(|e| e.members.clone());
                }
            }
        }
        self.type_members
            .classes
            .get(name)
            .map(|e| e.members.clone())
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = self
                    .resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))
                {
                    return rb.type_members.namespaces.get(name).cloned();
                }
            }
        }
        self.type_members.namespaces.get(name).cloned()
    }

    fn get_enum_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = self
                    .resolver
                    .module_bind(origin)
                    .or_else(|| self.resolver.stdlib_bind(origin))
                {
                    return rb.type_members.enums.get(name).cloned();
                }
            }
        }
        self.type_members.enums.get(name).cloned()
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        self.arena.get(id).ty.clone()
    }

    fn symbol_origin(&self, name: &str) -> Option<varn_core::Atom> {
        let scope = self.scopes.get(self.current);
        let atom = self.interner.get(name)?;
        let id = scope.resolve(atom, &self.scopes)?;
        self.arena.get(id).origin_module
    }

    fn source_file(&self) -> Option<&str> {
        Some(self.source_file.as_ref())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        let scope = self.scopes.get(self.current);
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
}

impl<'r> Binder<'r> {
    /// Mint a *new* `Atom` for text that isn't already in `self.interner`
    /// (a synthetic name like `"constructor"`, a symbol's `doc`, ...),
    /// keeping `self.interner` and the resolver's live, shared table from
    /// diverging.
    ///
    /// Why divergence is possible without this: binding an import can
    /// recurse into `bind_and_cache` for another module, which grows and
    /// *publishes* the live table (`DiskResolver::set_interner`) before
    /// returning — but `self.interner` is this `Binder`'s own snapshot,
    /// taken once at construction, and nothing about processing that import
    /// statement refreshes it. If this binder then locally mints an atom
    /// (`self.interner.intern(text)`) for the first time *after* that nested
    /// growth, it numbers the new atom starting from its own stale length —
    /// which can collide with whatever the nested bind already published at
    /// that same index. `DiskResolver::set_interner`'s prefix check is built
    /// to catch exactly this (a real bind observed it: two `Binder`s, one
    /// module importing another, both landed a different string on index 60)
    /// rather than let a `Symbol` silently resolve to someone else's text.
    ///
    /// The fix: before minting, adopt the live table if it has grown past
    /// what `self.interner` has (cheap to check via `interner_len`, only
    /// clones when actually behind) — safe because their shared prefix is
    /// never disputed, only what comes after it, so adopting a longer table
    /// is index-compatible; then also publish the *new* atom to the live
    /// table (`self.resolver.intern`), so a sibling `Binder`/nested bind
    /// still in flight sees it too. Both interns are deterministic
    /// dedup-then-append over the same prefix, so they agree on the index.
    pub(crate) fn intern_local(&mut self, text: &str) -> varn_core::Atom {
        self.resync_interner();
        let atom = self.interner.intern(text);
        self.resolver.intern(text);
        atom
    }

    /// The "adopt live if it's grown past us" half of [`Self::intern_local`],
    /// exposed on its own for a caller that's about to mint several atoms at
    /// once through code that doesn't go through `intern_local` itself (e.g.
    /// `cache::decode_symbol`'s several `interner.intern(text)` calls when
    /// rehydrating an imported symbol, `binder/imports.rs`) — one resync
    /// before the batch is enough, since nothing publishes to the live table
    /// *during* that batch (no resolver calls inside `decode_symbol`).
    pub(crate) fn resync_interner(&mut self) {
        if self.resolver.interner_len() > self.interner.len() {
            self.interner = self.resolver.interner_snapshot();
        }
    }

    /// Publish every text `self.interner` holds past the live table's length,
    /// keeping the two index-compatible.
    ///
    /// The companion of [`Self::resync_interner`] for batches that mint through
    /// `AtomInterner::intern` directly (e.g. `Symbol::from_cacheable`): the
    /// batch is preceded by one resync and there are no resolver calls inside
    /// it, so both tables append the same texts in the same order and the new
    /// indices agree. Without this the live table never sees the batch, and a
    /// later snapshot taken for a different module starts from a stale base —
    /// the divergence `intern_local` exists to prevent.
    pub(crate) fn publish_interner_tail(&mut self) {
        let live_len = self.resolver.interner_len();
        if self.interner.len() <= live_len {
            return;
        }
        let texts: Vec<String> = self
            .interner
            .iter_strings()
            .skip(live_len)
            .map(|s| s.to_owned())
            .collect();
        for text in texts {
            self.resolver.intern(&text);
        }
    }

    pub fn bind(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn ImportResolver,
    ) -> BindResult {
        Self::bind_with_globals_iter(program, ast_arena, interner, resolver, FxHashMap::default())
    }

    pub fn bind_with_global_refs(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn ImportResolver,
        globals: &FxHashMap<Arc<str>, Symbol>,
    ) -> BindResult {
        Self::bind_with_globals_iter(
            program,
            ast_arena,
            interner,
            resolver,
            globals
                .iter()
                .map(|(name, sym)| (name.clone(), sym.clone())),
        )
    }

    fn bind_with_globals_iter<I>(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn ImportResolver,
        globals: I,
    ) -> BindResult
    where
        I: IntoIterator<Item = (Arc<str>, Symbol)>,
    {
        let mut b = Binder {
            resolver,
            ast_arena,
            arena: SymbolArena::default(),
            scopes: ScopeArena::default(),
            current: 0,
            class_methods: FxHashMap::default(),
            type_members: TypeMembers::default(),
            class_parents: FxHashMap::default(),
            diagnostics: varn_core::DiagnosticBag::new(),
            interner,
            ty_table: resolver.ty_table_snapshot(),
            source_file: Arc::from(program.filename.as_ref()),
            sum_type_variants: FxHashMap::default(),
            sum_variant_parent: FxHashMap::default(),
            sum_variant_fields: FxHashMap::default(),
            extensions: Extensions::default(),
            pending_enrich: Vec::new(),
            reported_type_forms: Default::default(),
            array_watch: Vec::new(),
            evolved_array_types: FxHashMap::default(),
        };

        let global = b.scopes.push(CheckerScope::new(ScopeKind::Global, None));
        b.current = global;

        for (name, sym) in globals {
            let name_atom = b.interner.intern(name.as_ref());
            let id = b.arena.push(sym);
            b.scopes.get_mut(global).define(name_atom, id);
        }

        b.bind_stmts(&program.body);
        // The global scope never "exits" through a block, so finalize its
        // own candidates here — module-level `let x = []` is eligible.
        b.finalize_array_watch(global);

        BindResult {
            arena: b.arena,
            scopes: b.scopes,
            global_scope: global,
            diagnostics: b.diagnostics,
            interner: b.interner,
            ty_table: b.ty_table,
            class_methods: b.class_methods,
            type_members: b.type_members,
            class_parents: b.class_parents,
            source_file: b.source_file,
            sum_type_variants: b.sum_type_variants,
            sum_variant_parent: b.sum_variant_parent,
            sum_variant_fields: b.sum_variant_fields,
            extensions: b.extensions,
            core: None,
            pending_enrich: b.pending_enrich,
            evolved_array_types: b.evolved_array_types,
        }
    }

    pub(crate) fn bind_stmts(&mut self, stmts: &[StmtId]) {
        for &stmt in stmts {
            self.bind_stmt(stmt);
        }
    }

    /// `resolve_type_node(node, Some(self), &mut *std::sync::Arc::make_mut(&mut self.ty_table))` doesn't
    /// borrow-check: `Some(self)` takes `&Binder` (the whole struct, through
    /// the `dyn TypeContext` object) while `&mut *std::sync::Arc::make_mut(&mut self.ty_table)` needs a
    /// disjoint mutable borrow of one field, and a trait object erases the
    /// field-level information NLL would otherwise use to see they don't
    /// overlap. `resolve_type_node` only ever reads `table` through the
    /// explicit parameter (never through `ctx.ty_table()` — that accessor
    /// exists for callers with no such parameter to thread, like the
    /// checker's `compat` module), so swapping `self.ty_table` out for the
    /// call's duration is sound: nothing observes the gap.
    pub(crate) fn resolve_type(&mut self, node: &TypeNode) -> Type {
        self.reject_forbidden_type_forms(node);
        self.sync_ty_table();
        let mut table = std::mem::replace(
            std::sync::Arc::make_mut(&mut self.ty_table),
            crate::types::CheckerTyTable::default(),
        );
        let result = resolve_type_node(node, Some(self), &mut table);
        self.ty_table = std::sync::Arc::new(table);
        result
    }

    /// Spellings the language forbids outright, whatever they would resolve
    /// to: `Record<K, V>` (spec §22 — `#{…}` is the only record form).
    /// `resolve_type_node` resolves the form to `dynamic`, so no mismatch
    /// cascades from it.
    fn reject_forbidden_type_forms(&mut self, node: &TypeNode) {
        use varn_core::TypeKind as K;
        let children: Vec<&TypeNode> = match &node.kind {
            K::Generic(name, args, _) => {
                if self.interner.try_resolve(*name) == Some(varn_core::well_known::RECORD)
                    && self.reported_type_forms.insert(node.range.start.offset)
                {
                    self.emit(
                        varn_core::Diagnostic::error(
                            varn_core::ErrorCode::ForbiddenRecordGeneric,
                            "`Record<K, V>` is not a type: use `Map<K, V>` for a keyed collection or `{ [key: K]: V }` for an indexable object",
                        )
                        .with_range(node.range),
                    );
                }
                args.iter().collect()
            }
            K::Array(inner) | K::KeyOf(inner) => vec![inner.as_ref()],
            K::Union(list) | K::Intersection(list) | K::Tuple(list) => list.iter().collect(),
            K::Fn((params, ret)) => params
                .iter()
                .filter_map(|p| p.constraint.as_ref())
                .chain(std::iter::once(ret.as_ref()))
                .collect(),
            _ => vec![],
        };
        for child in children {
            self.reject_forbidden_type_forms(child);
        }
    }

    /// Same rationale as [`Self::resolve_type`], for `infer_expr_type`.
    pub(crate) fn infer_expr_type_self(&mut self, expr: varn_core::ast::ExprId) -> Type {
        self.sync_ty_table();
        let mut table = std::mem::replace(
            std::sync::Arc::make_mut(&mut self.ty_table),
            crate::types::CheckerTyTable::default(),
        );
        let arena = self.ast_arena;
        let result = infer_expr_type(expr, arena, Some(self), &mut table);
        self.ty_table = std::sync::Arc::new(table);
        result
    }

    /// Adopt the resolver's live `CheckerTyTable` when it has grown past this
    /// binder's snapshot. Nested imports bound during this bind publish new
    /// entries to the live table; a `Type` flowing back from such a module
    /// (global symbol, expanded alias, member type, …) can then carry a
    /// `CheckerTyId` past the end of the local snapshot, and the next
    /// `table.get(id)` indexes out of bounds.
    ///
    /// Merge, don't replace (see the `absorb` call in `check_internal` for
    /// why): the live table may have grown *independently* from this
    /// snapshot (same index, different shape), and a wholesale replacement
    /// would repoint every id this binder already minted.
    fn sync_ty_table(&mut self) {
        let live = self.resolver.ty_table_snapshot();
        if live.len() > self.ty_table.len() {
            std::sync::Arc::make_mut(&mut self.ty_table).absorb(&live);
        }
    }

    fn bind_var_declarators(
        &mut self,
        declarators: &[VarDeclarator],
        kind: VarKind,
        doc: Option<&Arc<str>>,
    ) {
        let sym_kind = match kind {
            VarKind::Const => SymbolKind::Const,
            VarKind::Let => SymbolKind::Let,
        };

        for declarator in declarators {
            let line = declarator.range.start.line;
            let ty = declarator
                .type_ann
                .as_ref()
                .or(match &declarator.id {
                    Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                    _ => None,
                })
                .map(|ann| self.resolve_type(ann))
                .or_else(|| {
                    declarator
                        .init
                        .map(|expr| self.infer_expr_type_self(expr))
                        .filter(|ty| !ty.is_dynamic())
                });

            self.bind_pattern(
                &declarator.id,
                sym_kind,
                line,
                doc.as_ref().map(|s| s.to_string()),
                ty,
            );

            if let Pattern::Identifier { name, .. } = &declarator.id {
                if let Some(init_expr) = declarator.init {
                    if let ExprKind::Object { properties, .. } =
                        &self.ast_arena.expr(init_expr).kind
                    {
                        let fields = self.collect_object_members(properties);
                        if !fields.is_empty() {
                            self.type_members.objects.insert(name.clone(), fields);
                        }
                    }
                }
            }

            if let Some(init_expr) = declarator.init {
                self.bind_expr(init_expr);
            }
        }
    }

    pub(crate) fn bind_stmt(&mut self, stmt: StmtId) {
        let arena = self.ast_arena;
        match &arena.stmt(stmt).kind {
            StmtKind::Decl(decl) => self.bind_decl(decl),
            StmtKind::Block { stmts } => {
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                self.bind_stmts(stmts);
                self.finalize_array_watch(child);
                self.current = saved;
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
                self.bind_expr(test);
                self.bind_stmt(consequent);
                if let Some(alt) = alternate {
                    self.bind_stmt(alt);
                }
            }
            StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
                let (test, body) = (*test, *body);
                self.bind_expr(test);
                self.bind_stmt(body);
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                let body = *body;
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                if let Some(init) = init {
                    match init.as_ref() {
                        ForInit::Var { kind, declarators } => {
                            self.bind_var_declarators(declarators, *kind, None);
                        }
                        ForInit::Expr(e) => {
                            self.bind_expr(*e);
                        }
                    }
                }
                if let Some(t) = test {
                    self.bind_expr(*t);
                }
                if let Some(u) = update {
                    self.bind_expr(*u);
                }
                self.bind_stmt(body);
                self.finalize_array_watch(child);
                self.current = saved;
            }
            StmtKind::ForIn {
                left, right, body, ..
            }
            | StmtKind::ForOf {
                left, right, body, ..
            } => {
                let (right, body) = (*right, *body);
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                let line = arena.expr(right).range.start.line;
                self.bind_pattern(left, SymbolKind::Let, line, None, None);
                self.bind_expr(right);
                self.bind_stmt(body);
                self.finalize_array_watch(child);
                self.current = saved;
            }
            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                let discriminant = *discriminant;
                self.bind_expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.bind_expr(*t);
                    }
                    self.bind_stmts(&case.body);
                }
            }
            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                let (block, finally) = (*block, *finally);
                self.bind_stmt(block);
                for clause in catches {
                    let child = self.scopes.child(ScopeKind::Block, self.current);
                    let saved = self.current;
                    self.current = child;
                    if let Some(p) = &clause.param {
                        let ty = clause.type_ann.as_ref().map(|ann| self.resolve_type(ann));
                        let block_line = arena.stmt(block).range.start.line;
                        self.bind_pattern(p, SymbolKind::Let, block_line, None, ty);
                    }
                    self.bind_stmt(clause.body);
                    self.finalize_array_watch(child);
                    self.current = saved;
                }
                if let Some(fin) = finally {
                    self.bind_stmt(fin);
                }
            }
            StmtKind::Labeled { body, .. } => {
                self.bind_stmt(*body);
            }
            StmtKind::Expr { expression } => {
                self.bind_expr(*expression);
            }
            StmtKind::Return { argument } => {
                if let Some(arg) = argument {
                    self.bind_expr(*arg);
                }
            }
            StmtKind::Throw { argument } => {
                self.bind_expr(*argument);
            }
            StmtKind::Using { declarations, .. } => {
                self.bind_var_declarators(declarations, VarKind::Const, None);
            }
            _ => {}
        }
    }
}
