use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use crate::module_resolver::graph::ModuleGraph;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use varn_core::ModuleId;

type CoreExportsMap = rustc_hash::FxHashMap<Rc<str>, crate::symbol::Symbol>;

/// How the checker reaches other modules.
///
/// The checker asks questions ("what does this module export?", "what did it
/// bind?") and never decides where the answers come from. Making that an
/// injected capability instead of ambient state is what turns `check` into a
/// function of its arguments: whoever owns the module graph — a language
/// server's query engine, the CLI's disk resolver — passes it in.
///
/// The old arrangement kept the graph in `thread_local!` statics inside this
/// crate, so `invalidate_module_cache()` cleared only the calling thread and
/// every other worker in a pool kept stale binds. A checker that owns no cache
/// has nothing to go stale.
///
/// All methods take `&self`: implementations that memoize use interior
/// mutability, and must not hold a borrow across a call back into the resolver,
/// because export resolution is mutually recursive through imports.
pub trait ImportResolver {
    /// Bind a workspace module identified by canonical absolute path.
    fn module_bind(&self, abs_path: &str) -> Option<Rc<BindResult>>;

    /// Exports of a workspace module. `visiting` carries the in-progress cycle
    /// set; import cycles resolve to an empty map rather than recursing.
    fn module_exports(&self, abs_path: &str, visiting: &mut Vec<String>) -> Rc<ExportMap>;

    /// Bind a `std:` / `core:` / `runtime:` module.
    fn stdlib_bind(&self, specifier: &str) -> Option<Rc<BindResult>>;

    /// Exports of a `std:` / `core:` / `runtime:` module.
    fn stdlib_exports(&self, specifier: &str) -> Rc<ExportMap>;

    /// Turn an import specifier into an absolute path, relative to `base_dir`.
    fn resolve_specifier(&self, base_dir: &Path, specifier: &str) -> Option<String>;

    /// Note that `importer` depends on `imported`, so invalidating the latter
    /// can evict the former.
    fn record_dep(&self, importer: &str, imported: &str);

    /// The prelude's global symbols (`core:*`), as this resolver's stdlib
    /// defines them. Part of the trait because which prelude is in force is a
    /// property of which stdlib you resolve against.
    fn core_exports(&self) -> Rc<rustc_hash::FxHashMap<Rc<str>, crate::symbol::Symbol>>;

    /// A clone of this resolver's single, whole-compilation `Atom` table.
    ///
    /// `core_exports`'s `Symbol`s carry `Atom`s minted while binding the core
    /// stdlib modules against this same table — a caller that binds a program
    /// against a `Checker::check`-supplied `AtomInterner` captured *before*
    /// `core_exports()` ran must re-fetch this snapshot afterward, or those
    /// `Symbol`s' `Atom`s (now real, published indices) resolve out of bounds
    /// against the caller's now-stale, smaller table.
    fn interner_snapshot(&self) -> varn_core::AtomInterner;

    /// A clone of this resolver's single, whole-compilation `CheckerTyId`
    /// table, mirroring [`Self::interner_snapshot`] for exactly the same
    /// reason: a module imports types from another module, so their
    /// `CheckerTyId`s must be comparable — which only holds if every
    /// `Binder` grows the SAME numbering from a prefix-compatible snapshot
    /// of it, never a table of its own.
    fn ty_table_snapshot(&self) -> std::sync::Arc<crate::types::CheckerTyTable>;

    /// Publish `table`'s shapes into the compilation's live `CheckerTyId`
    /// table. Merging, not replacing: `table` is one module's locally-grown
    /// view, which can disagree with the live table past their common prefix
    /// (nested binds grow live behind any single module's back), and a
    /// wholesale swap would repoint every id the live table already handed
    /// out. `CheckerTyTable::absorb` keeps live's own indices stable and only
    /// learns shapes it is missing.
    fn set_ty_table(&self, table: std::sync::Arc<crate::types::CheckerTyTable>);

    /// Intern `kind` into the live `CheckerTyId` table itself, publishing
    /// immediately, and return the id it now has *there*.
    ///
    /// For a caller that must put a type on an exported symbol (`export * as
    /// ns`, a synthesized member) without growing any module's bind table —
    /// the id crosses module boundaries through the `ExportMap`, so it has to
    /// be valid in the table importers decode against, which is the live one.
    fn intern_ty(&self, kind: crate::types::InternedTypeKind) -> crate::types::CheckerTyId;

    /// Intern `s` into this resolver's shared `Atom` table, publishing the
    /// result immediately (unlike `interner_snapshot`, which only reads).
    ///
    /// Exists for callers that hold no mutable interner of their own but must
    /// mint an `Atom` for text that is not part of any module's own source —
    /// an absolute file path or `std:`-style specifier used as an export's
    /// `origin_module`. Interning here (the live, shared table) rather than
    /// into a throwaway copy is what makes the returned `Atom` resolve
    /// correctly through every later `interner_snapshot()`.
    fn intern(&self, s: &str) -> varn_core::Atom;

    /// Length of this resolver's live `Atom` table, without cloning it (unlike
    /// [`Self::interner_snapshot`]). Lets a caller cheaply check "has the live
    /// table grown past what I have locally" before paying for a snapshot —
    /// see `Binder::intern_local`'s doc for why that check has to happen
    /// before every locally-minted `Atom`, not just at construction.
    fn interner_len(&self) -> usize;

    /// The prelude's member tables.
    fn core_members(&self) -> Rc<crate::core::loader::CoreMembers>;

    /// Bind whichever of `origin_modules` actually declares `type_name`.
    ///
    /// A module that fails to resolve is skipped, not fatal: the caller is
    /// asking "which of these declares it", and one unreadable candidate says
    /// nothing about the rest.
    ///
    /// Derived from the primitives above, so implementations inherit it.
    fn find_bind_for_type(
        &self,
        type_name: &str,
        origin_modules: &[String],
    ) -> Option<Rc<BindResult>> {
        for path in origin_modules {
            let Some(bind) = self.module_bind(path).or_else(|| self.stdlib_bind(path)) else {
                continue;
            };
            if bind.get_class_entry(type_name).is_some()
                || bind.get_namespace_members_local(type_name).is_some()
                || bind.get_interface_members_local(type_name).is_some()
            {
                return Some(bind);
            }
        }
        None
    }
}

/// The resolver that reads modules through the canonical
/// [`varn_modules::loader::ModuleRegistry`] (filesystem + builtins/std provider)
/// and memoizes them in a [`ModuleGraph`] it owns.
///
/// One of these per workspace. The `RefCell` is an implementation detail of an
/// object whose lifetime the caller controls — not a process-lifetime global —
/// which is the whole difference from what this replaces.
pub struct DiskResolver {
    /// The single loader: source for every module (file, bundle, provider)
    /// comes from here. This resolver does not read files or talk to the
    /// provider itself — see ADR-0011.
    loader: varn_modules::loader::ModuleRegistry,
    graph: RefCell<ModuleGraph>,
    /// Modules whose binding is currently on the stack. Used to break
    /// mutual-import deadlocks when a module's body imports a peer that then
    /// asks for `core:types` again to expand a generic alias.
    in_flight: RefCell<rustc_hash::FxHashSet<String>>,
    /// The prelude, derived once from the stdlib this resolver serves.
    ///
    /// Lives here rather than in a process-wide static because it is a
    /// *function of the stdlib in use*: a process that switches std provenance
    /// (the language server does, between the checkout tree and the embedded
    /// bundle) would otherwise keep answering from the first one it ever saw.
    core_exports: RefCell<Option<Rc<CoreExportsMap>>>,
    core_members: RefCell<Option<Rc<crate::core::loader::CoreMembers>>>,
    /// The single `Atom` table for this compilation. Every `varn_parser::parse`
    /// this resolver drives (the entry file included, via
    /// `interner_snapshot`/`set_interner`) reads from and grows this same
    /// table, so an `Atom` minted while parsing one module compares equal to
    /// the same text minted while parsing another — see `Symbol::origin_module`.
    /// A resolver-per-file `AtomInterner` was the bug: two parses never shared
    /// one, so their `Atom` indices meant nothing to each other.
    interner: RefCell<varn_core::AtomInterner>,
    /// The single `CheckerTyId` table for this compilation — same reasoning
    /// and lifecycle as `interner` above (see `ImportResolver::ty_table_snapshot`).
    ty_table: RefCell<std::sync::Arc<crate::types::CheckerTyTable>>,
}

impl Default for DiskResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskResolver {
    pub fn new() -> Self {
        Self {
            loader: varn_modules::loader::default_registry(),
            graph: RefCell::default(),
            in_flight: RefCell::default(),
            core_exports: RefCell::default(),
            core_members: RefCell::default(),
            interner: RefCell::default(),
            ty_table: RefCell::default(),
        }
    }

    /// The one way this resolver obtains a module's source or precomputed
    /// artifacts.
    fn load_source(&self, id: &ModuleId) -> Option<varn_modules::loader::ModuleSource> {
        use varn_modules::loader::ModuleLoader;
        self.loader.source(id).ok()
    }

    /// Mutate the live `CheckerTyTable` in place. Used by the interface cache
    /// to decode into the SAME table consumers will read from, so decoded ids
    /// are valid for everyone (ADR-0011, Ley 2).
    pub(crate) fn with_ty_table_mut<R>(
        &self,
        f: impl FnOnce(&mut crate::types::CheckerTyTable) -> R,
    ) -> R {
        let mut live = self.ty_table.borrow_mut();
        f(std::sync::Arc::make_mut(&mut live))
    }

    /// A clone of the compilation's `Atom` table as of now. Cheap relative to
    /// a parse, and the only way to hand modules-so-far's interned text to a
    /// caller without exposing the `RefCell` itself: `AtomInterner::clone`
    /// copies the dedup map and string vec, but every `Atom` it already
    /// contains keeps the same index, so an atom resolved through this clone
    /// resolves identically through the resolver's live table or through any
    /// other snapshot taken later.
    pub fn interner_snapshot(&self) -> varn_core::AtomInterner {
        self.interner.borrow().clone()
    }

    /// Publish `interner` into the compilation's shared table: keep every
    /// entry the live table already has (its indices are the ones already
    /// minted `Atom`s refer to) and append the texts the incoming table has
    /// past the live length.
    ///
    /// Not a wholesale replacement: a bind that recurses into another
    /// module's import snapshots, grows, and publishes on its own schedule,
    /// so two binders can grow *independently* from a common snapshot and
    /// land different texts past the shared prefix (observed: a file-path
    /// atom vs `__ext_str_shout` at index 813). Overwriting live with the
    /// incoming table would repoint every `Atom` the live table already
    /// handed out; a debug-only prefix-equality panic is no better — the
    /// divergence is a consequence of the snapshot design, not a caller bug.
    /// Appending (dedup-by-text) keeps live append-only and every already-
    /// minted index stable; symbols carrying a diverged binder's local atom
    /// still resolve through that binder's own interner for cross-module
    /// reads (the `cache::encode_symbol`/`decode_symbol` text round-trip).
    pub fn set_interner(&self, interner: varn_core::AtomInterner) {
        let mut live = self.interner.borrow_mut();
        if interner.len() <= live.len() {
            return;
        }
        let new_texts: Vec<String> = interner
            .iter_strings()
            .skip(live.len())
            .map(|s| s.to_owned())
            .collect();
        for text in new_texts {
            live.intern(&text);
        }
    }

    /// Evict `id` and everything that transitively imports it.
    pub fn invalidate(&self, id: &ModuleId) {
        self.graph.borrow_mut().invalidate(id);
    }

    /// Drop every memoized module, the prelude included: a std swap invalidates
    /// it just as surely as an edit invalidates a workspace module.
    pub fn clear(&self) {
        self.graph.borrow_mut().clear();
        *self.core_exports.borrow_mut() = None;
        *self.core_members.borrow_mut() = None;
    }

    pub fn types_cache_dir(&self) -> std::path::PathBuf {
        // Clone the root and drop the borrow before calling out. Resolution is
        // re-entrant, and a borrow spanning a call into another crate is the
        // kind of thing that only fails once someone makes that crate call
        // back.
        let root = self.graph.borrow_mut().project_root_or_init().clone();
        varn_modules::artifact::get_types_cache_dir(&root)
    }

    // ── graph access ─────────────────────────────────────────────────────
    //
    // Each of these takes and releases the borrow immediately. Resolution is
    // mutually recursive through imports, so a borrow held across a nested
    // resolve would panic at runtime.

    pub(super) fn cached_bind(&self, key: &str) -> Option<Rc<BindResult>> {
        self.graph.borrow().bind(key)
    }

    pub(super) fn store_bind(&self, key: String, bind: Rc<BindResult>) {
        self.graph.borrow_mut().insert_bind(key, bind);
    }

    pub(super) fn cached_exports(&self, key: &str) -> Option<Rc<ExportMap>> {
        self.graph.borrow().exports(key)
    }

    pub(super) fn store_exports(&self, key: String, exports: Rc<ExportMap>) {
        self.graph.borrow_mut().insert_exports(key, exports);
    }

    pub(super) fn cached_program(&self, key: &str) -> Option<Rc<varn_core::ast::Program>> {
        self.graph.borrow().program(key)
    }

    pub(super) fn store_program(&self, key: String, program: Rc<varn_core::ast::Program>) {
        self.graph.borrow_mut().insert_program(key, program);
    }

    pub(super) fn cached_arena(&self, key: &str) -> Option<Rc<varn_core::ast::AstArena>> {
        self.graph.borrow().arena(key)
    }

    pub(super) fn store_arena(&self, key: String, arena: Rc<varn_core::ast::AstArena>) {
        self.graph.borrow_mut().insert_arena(key, arena);
    }

    pub(super) fn cached_path(&self, base_dir: &str, specifier: &str) -> Option<String> {
        self.graph.borrow().resolved_path(base_dir, specifier)
    }

    pub(super) fn store_path(&self, base_dir: String, specifier: String, abs: String) {
        self.graph
            .borrow_mut()
            .insert_resolved_path(base_dir, specifier, abs);
    }

    // ── parsing and binding ──────────────────────────────────────────────

    fn parse_and_cache(
        &self,
        source: &str,
        key: &str,
    ) -> Option<(
        Rc<varn_core::ast::Program>,
        Rc<varn_core::ast::AstArena>,
        Vec<varn_core::Diagnostic>,
    )> {
        let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, key);
        // Seed this parse from a clone of the shared table rather than handing
        // it out by value: on a parse error the clone is simply dropped and
        // the resolver's own table is untouched, so a module that fails to
        // parse never rolls back atoms other modules already minted.
        let interner = self.interner_snapshot();
        let (program, interner, arena) =
            varn_parser::parse(tokens, lexeme_buf, key, interner).ok()?;
        self.set_interner(interner);
        let program = Rc::new(program);
        let arena = Rc::new(arena);
        self.store_program(key.to_owned(), Rc::clone(&program));
        self.store_arena(key.to_owned(), Rc::clone(&arena));
        Some((program, arena, lex_errs))
    }

    /// True while `key`'s bind is in progress; see [`DiskResolver::in_flight`].
    pub(super) fn is_binding(&self, key: &str) -> bool {
        self.in_flight.borrow().contains(key)
    }

    fn bind_and_cache(
        &self,
        program: &varn_core::ast::Program,
        ast_arena: &varn_core::ast::AstArena,
        interner: varn_core::AtomInterner,
        lex_errs: Vec<varn_core::Diagnostic>,
        key: &str,
    ) -> Rc<BindResult> {
        self.in_flight.borrow_mut().insert(key.to_owned());
        let mut bind = crate::binder::Binder::bind(program, ast_arena, interner, self);
        self.in_flight.borrow_mut().remove(key);
        for e in lex_errs {
            bind.diagnostics.emit(e);
        }
        // Binding itself coins new atoms (doc comments, "constructor", "this",
        // mangled extension names, ...) on top of whatever parsing produced.
        // Without publishing them back, a later `interner_snapshot()` (e.g.
        // `save_to_cache`) resolves against a table that never saw them and
        // panics out of bounds — same fix as `parse_and_cache`.
        self.set_interner(bind.interner.clone());
        self.set_ty_table(bind.ty_table.clone());
        let bind = Rc::new(bind);
        self.store_bind(key.to_owned(), Rc::clone(&bind));
        bind
    }

    /// Collect a program's exports, resolving its own imports through `self`.
    fn collect(
        &self,
        program: &varn_core::ast::Program,
        ast_arena: &varn_core::ast::AstArena,
        bind: &BindResult,
        key: &str,
        base_dir: &Path,
        visiting: &mut Vec<String>,
    ) -> ExportMap {
        let mut exports = ExportMap::default();
        super::exports::collect_exports(
            self,
            &program.body,
            ast_arena,
            bind,
            key,
            base_dir,
            visiting,
            &mut exports,
        );
        super::exports::assign_slots(&mut exports);
        exports
    }

    /// Exports of an already-bound or freshly-read workspace module.
    fn module_exports_uncached(&self, abs_path: &str, visiting: &mut Vec<String>) -> ExportMap {
        let base_dir = Path::new(abs_path).parent().unwrap_or(Path::new("."));

        if let (Some(bind), Some(program), Some(ast_arena)) = (
            self.cached_bind(abs_path),
            self.cached_program(abs_path),
            self.cached_arena(abs_path),
        ) {
            return self.collect(
                &program,
                ast_arena.as_ref(),
                bind.as_ref(),
                abs_path,
                base_dir,
                visiting,
            );
        }

        let Some(source) = self.load_source(&ModuleId::local_str(abs_path)) else {
            return ExportMap::default();
        };
        let source = source.text;
        let Some((program, ast_arena, _lex_errs)) = self.parse_and_cache(&source, abs_path)
        else {
            return ExportMap::default();
        };
        let bind = self.cached_bind(abs_path).unwrap_or_else(|| {
            self.bind_and_cache(
                &program,
                ast_arena.as_ref(),
                self.interner_snapshot(),
                Vec::new(),
                abs_path,
            )
        });

        self.collect(
            &program,
            ast_arena.as_ref(),
            bind.as_ref(),
            abs_path,
            base_dir,
            visiting,
        )
    }

    // ── carga de stdlib (a través del loader único) ──────────────────────

    fn exports_from_embedded(
        &self,
        virtual_id: &str,
        source: &str,
        carrier: super::CarrierKind,
        visiting: &mut Vec<String>,
    ) -> Rc<ExportMap> {
        if visiting.iter().any(|v| v == virtual_id) {
            return Rc::new(ExportMap::default());
        }
        visiting.push(virtual_id.to_owned());

        if let Some(cached) = super::cache::try_load_cache(self, virtual_id, source, carrier) {
            self.store_bind(virtual_id.to_owned(), Rc::new(cached.bind));
            visiting.pop();
            return Rc::new(cached.exports);
        }

        let Some((program, ast_arena, _lex_errs)) = self.parse_and_cache(source, virtual_id)
        else {
            visiting.pop();
            return Rc::new(ExportMap::default());
        };
        let bind = self.bind_and_cache(
            &program,
            ast_arena.as_ref(),
            self.interner_snapshot(),
            Vec::new(),
            virtual_id,
        );
        let exports = self.collect(
            &program,
            ast_arena.as_ref(),
            bind.as_ref(),
            virtual_id,
            Path::new("."),
            visiting,
        );

        super::cache::save_to_cache(self, virtual_id, source, &exports, bind.as_ref(), carrier);
        visiting.pop();
        Rc::new(exports)
    }

    fn bind_from_embedded(
        &self,
        virtual_id: &str,
        source: &str,
        carrier: super::CarrierKind,
    ) -> Option<Rc<BindResult>> {
        if let Some(cached) = self.cached_bind(virtual_id) {
            return Some(cached);
        }
        if let Some(cached) = super::cache::try_load_cache(self, virtual_id, source, carrier) {
            let bind_rc = Rc::new(cached.bind);
            self.store_bind(virtual_id.to_owned(), Rc::clone(&bind_rc));
            self.store_exports(
                ModuleId::stdlib(virtual_id).as_str().to_owned(),
                Rc::new(cached.exports),
            );
            return Some(bind_rc);
        }
        let (program, ast_arena, lex_errs) = self.parse_and_cache(source, virtual_id)?;
        Some(self.bind_and_cache(
            &program,
            ast_arena.as_ref(),
            self.interner_snapshot(),
            lex_errs,
            virtual_id,
        ))
    }
}

impl ImportResolver for DiskResolver {
    fn interner_snapshot(&self) -> varn_core::AtomInterner {
        self.interner.borrow().clone()
    }

    fn ty_table_snapshot(&self) -> std::sync::Arc<crate::types::CheckerTyTable> {
        self.ty_table.borrow().clone()
    }

    fn set_ty_table(&self, table: std::sync::Arc<crate::types::CheckerTyTable>) {
        // Merge, never replace: `table` is one module's locally-grown view,
        // which can disagree with the live table past their common prefix.
        // `absorb` keeps live's own indices stable and only learns shapes it
        // is missing.
        let mut live = self.ty_table.borrow_mut();
        std::sync::Arc::make_mut(&mut live).absorb(&table);
    }

    fn intern_ty(&self, kind: crate::types::InternedTypeKind) -> crate::types::CheckerTyId {
        let mut live = self.ty_table.borrow_mut();
        std::sync::Arc::make_mut(&mut live).intern(kind)
    }

    fn interner_len(&self) -> usize {
        self.interner.borrow().len()
    }

    fn intern(&self, s: &str) -> varn_core::Atom {
        self.interner.borrow_mut().intern(s)
    }

    fn module_bind(&self, abs_path: &str) -> Option<Rc<BindResult>> {
        if let Some(cached) = self.cached_bind(abs_path) {
            return Some(cached);
        }

        let canonical = varn_modules::canonical_or_original(Path::new(abs_path));
        // Look again under the canonical key. Callers reach this with whatever
        // spelling the type's `origin` carries -- on Windows that is the
        // extended form, `\\?\C:\...\m.vn`, while every store below writes
        // `C:/.../m.vn`. Checking only `abs_path` made the memo permanently
        // cold for those callers: each one re-read the file and re-hashed the
        // on-disk cache to rebuild a `BindResult` already sitting in the graph.
        // `module_exports` has always done this; `module_bind` had not.
        if canonical != abs_path {
            if let Some(cached) = self.cached_bind(&canonical) {
                return Some(cached);
            }
        }

        let Some(source) = self.load_source(&ModuleId::local_str(&canonical)) else {
            return None;
        };
        let carrier = super::CarrierKind::from(source.provenance);
        let source = source.text;
        let source = source.as_ref();

        if let Some(cached) = super::cache::try_load_cache(self, &canonical, source, carrier) {
            let bind_rc = Rc::new(cached.bind);
            self.store_bind(canonical.clone(), Rc::clone(&bind_rc));
            self.store_exports(canonical, Rc::new(cached.exports));
            return Some(bind_rc);
        }

        let (program, ast_arena, lex_errs) = self.parse_and_cache(source, &canonical)?;
        let bind = self.bind_and_cache(
            &program,
            ast_arena.as_ref(),
            self.interner_snapshot(),
            lex_errs,
            &canonical,
        );

        let base_dir = Path::new(&canonical).parent().unwrap_or(Path::new("."));
        let exports = self.collect(
            &program,
            ast_arena.as_ref(),
            bind.as_ref(),
            &canonical,
            base_dir,
            &mut Vec::new(),
        );
        super::cache::save_to_cache(self, &canonical, source, &exports, bind.as_ref(), carrier);

        Some(bind)
    }

    fn module_exports(&self, abs_path: &str, visiting: &mut Vec<String>) -> Rc<ExportMap> {
        if let Some(cached) = self.cached_exports(abs_path) {
            return cached;
        }

        let canonical = varn_modules::canonical_or_original(Path::new(abs_path));
        if canonical != abs_path {
            if let Some(cached) = self.cached_exports(&canonical) {
                return cached;
            }
        }

        if visiting.iter().any(|v| v == &canonical) {
            return Rc::new(ExportMap::default());
        }

        // Publish an empty map before recursing: a cycle that reaches this
        // module again finds the sentinel instead of recursing forever.
        self.store_exports(canonical.clone(), Rc::new(ExportMap::default()));

        visiting.push(canonical.clone());
        let result = Rc::new(self.module_exports_uncached(&canonical, visiting));
        visiting.pop();

        self.store_exports(canonical, Rc::clone(&result));
        result
    }

    fn stdlib_bind(&self, specifier: &str) -> Option<Rc<BindResult>> {
        let key = ModuleId::stdlib(specifier).as_str();
        if let Some(cached) = self.cached_bind(&key) {
            return Some(cached);
        }
        if self.is_binding(&key) || self.is_binding(specifier) {
            return None;
        }

        let source = self.load_source(&ModuleId::stdlib(specifier))?;
        let carrier = super::CarrierKind::from(source.provenance);
        // SOURCE es la verdad; la interfaz precompilada es una optimización
        // (ver ADR-0011). Se carga desde texto siempre que exista, para que el
        // checker y el VM vean las mismas bytes para el mismo `ModuleId`.
        self.bind_from_embedded(specifier, source.text.as_ref(), carrier)
    }

    fn stdlib_exports(&self, specifier: &str) -> Rc<ExportMap> {
        let key = ModuleId::stdlib(specifier).as_str();
        if let Some(cached) = self.cached_exports(&key) {
            return cached;
        }

        let result = self
            .load_source(&ModuleId::stdlib(specifier))
            .map(|source| {
                let carrier = super::CarrierKind::from(source.provenance);
                self.exports_from_embedded(
                    specifier,
                    source.text.as_ref(),
                    carrier,
                    &mut Vec::new(),
                )
            });

        match result {
            Some(exports) => {
                self.store_exports(key, Rc::clone(&exports));
                exports
            }
            None => Rc::new(ExportMap::default()),
        }
    }

    fn resolve_specifier(&self, base_dir: &Path, specifier: &str) -> Option<String> {
        let base_str = base_dir.to_string_lossy().into_owned();
        if let Some(hit) = self.cached_path(&base_str, specifier) {
            return Some(hit);
        }
        let resolved = varn_modules::resolver::resolve_specifier_path(base_dir, specifier)?;
        self.store_path(base_str, specifier.to_owned(), resolved.clone());
        Some(resolved)
    }

    fn record_dep(&self, importer: &str, imported: &str) {
        self.graph.borrow_mut().record_dep(importer, imported);
    }

    fn core_exports(&self) -> Rc<rustc_hash::FxHashMap<Rc<str>, crate::symbol::Symbol>> {
        if let Some(hit) = self.core_exports.borrow().as_ref() {
            return Rc::clone(hit);
        }
        // Built with the borrow released: building resolves stdlib modules
        // through `self`, which takes the same borrows.
        let built = Rc::new(crate::core::loader::build_core_exports(self));
        *self.core_exports.borrow_mut() = Some(Rc::clone(&built));
        built
    }

    fn core_members(&self) -> Rc<crate::core::loader::CoreMembers> {
        if let Some(hit) = self.core_members.borrow().as_ref() {
            return Rc::clone(hit);
        }
        let built = Rc::new(crate::core::loader::build_core_members(self));
        *self.core_members.borrow_mut() = Some(Rc::clone(&built));
        built
    }
}
