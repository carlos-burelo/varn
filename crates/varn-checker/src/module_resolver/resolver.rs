use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use crate::module_resolver::graph::ModuleGraph;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use varn_core::ModuleId;

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

/// The resolver that reads modules from disk (or from the active stdlib
/// provider) and memoizes them in a [`ModuleGraph`] it owns.
///
/// One of these per workspace. The `RefCell` is an implementation detail of an
/// object whose lifetime the caller controls — not a process-lifetime global —
/// which is the whole difference from what this replaces.
#[derive(Default)]
pub struct DiskResolver {
    graph: RefCell<ModuleGraph>,
    /// Modules whose bind is currently being computed.
    ///
    /// A module is only inserted into the graph *after* it binds, so a module
    /// that re-enters resolution while binding itself would recurse forever.
    /// `core:types` does exactly that: binding it resolves type nodes, which
    /// asks for `core:types` again to expand a generic alias.
    in_flight: RefCell<std::collections::HashSet<String>>,
    /// The prelude, derived once from the stdlib this resolver serves.
    ///
    /// Lives here rather than in a process-wide static because it is a
    /// *function of the stdlib in use*: a process that switches std provenance
    /// (the language server does, between the checkout tree and the embedded
    /// bundle) would otherwise keep answering from the first one it ever saw.
    core_exports: RefCell<Option<Rc<rustc_hash::FxHashMap<Rc<str>, crate::symbol::Symbol>>>>,
    core_members: RefCell<Option<Rc<crate::core::loader::CoreMembers>>>,
}

impl DiskResolver {
    pub fn new() -> Self {
        Self::default()
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
    ) -> Option<(Rc<varn_core::ast::Program>, Vec<varn_core::Diagnostic>)> {
        let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, key);
        let program = Rc::new(varn_parser::parse(tokens, lexeme_buf, key).ok()?);
        self.store_program(key.to_owned(), Rc::clone(&program));
        Some((program, lex_errs))
    }

    /// True while `key`'s bind is in progress; see [`DiskResolver::in_flight`].
    pub(super) fn is_binding(&self, key: &str) -> bool {
        self.in_flight.borrow().contains(key)
    }

    fn bind_and_cache(
        &self,
        program: &varn_core::ast::Program,
        lex_errs: Vec<varn_core::Diagnostic>,
        key: &str,
    ) -> Rc<BindResult> {
        self.in_flight.borrow_mut().insert(key.to_owned());
        let mut bind = crate::binder::Binder::bind(program, self);
        self.in_flight.borrow_mut().remove(key);
        for e in lex_errs {
            bind.diagnostics.emit(e);
        }
        let bind = Rc::new(bind);
        self.store_bind(key.to_owned(), Rc::clone(&bind));
        bind
    }

    /// Collect a program's exports, resolving its own imports through `self`.
    fn collect(
        &self,
        program: &varn_core::ast::Program,
        bind: &BindResult,
        key: &str,
        base_dir: &Path,
        visiting: &mut Vec<String>,
    ) -> ExportMap {
        let mut exports = ExportMap::default();
        super::exports::collect_exports(
            self, &program.body, bind, key, base_dir, visiting, &mut exports,
        );
        super::exports::assign_slots(&mut exports);
        exports
    }

    /// Exports of an already-bound or freshly-read workspace module.
    fn module_exports_uncached(&self, abs_path: &str, visiting: &mut Vec<String>) -> ExportMap {
        let base_dir = Path::new(abs_path).parent().unwrap_or(Path::new("."));

        if let (Some(bind), Some(program)) =
            (self.cached_bind(abs_path), self.cached_program(abs_path))
        {
            return self.collect(&program, bind.as_ref(), abs_path, base_dir, visiting);
        }

        let Ok(source) = std::fs::read_to_string(abs_path) else {
            return ExportMap::default();
        };
        let Some((program, _lex_errs)) = self.parse_and_cache(&source, abs_path) else {
            return ExportMap::default();
        };
        let bind = self
            .cached_bind(abs_path)
            .unwrap_or_else(|| self.bind_and_cache(&program, Vec::new(), abs_path));

        self.collect(&program, bind.as_ref(), abs_path, base_dir, visiting)
    }

    // ── stdlib carriers ──────────────────────────────────────────────────

    fn from_interface_blob(
        &self,
        specifier: &str,
        key: &str,
    ) -> Option<(Rc<ExportMap>, Rc<BindResult>)> {
        let provider = varn_modules::provider::get()?;
        let blob = provider.interface_blob(specifier)?;
        match super::cache::deserialize_module_interface(blob) {
            Ok((exports, bind)) => {
                let exports = Rc::new(exports);
                let bind = Rc::new(bind);
                self.store_exports(key.to_owned(), Rc::clone(&exports));
                self.store_bind(key.to_owned(), Rc::clone(&bind));
                Some((exports, bind))
            }
            Err(e) => panic!("corrupt interface blob for {specifier}: {e}"),
        }
    }

    fn exports_from_embedded(
        &self,
        virtual_id: &str,
        source: &str,
        visiting: &mut Vec<String>,
    ) -> Rc<ExportMap> {
        if visiting.iter().any(|v| v == virtual_id) {
            return Rc::new(ExportMap::default());
        }
        visiting.push(virtual_id.to_owned());

        if let Some(cached) = super::cache::try_load_cache(self, virtual_id, source) {
            self.store_bind(virtual_id.to_owned(), Rc::new(cached.bind));
            visiting.pop();
            return Rc::new(cached.exports);
        }

        let Some((program, _lex_errs)) = self.parse_and_cache(source, virtual_id) else {
            visiting.pop();
            return Rc::new(ExportMap::default());
        };
        let bind = self.bind_and_cache(&program, Vec::new(), virtual_id);
        let exports = self.collect(&program, bind.as_ref(), virtual_id, Path::new("."), visiting);

        super::cache::save_to_cache(self, virtual_id, source, &exports, bind.as_ref());
        visiting.pop();
        Rc::new(exports)
    }

    fn bind_from_embedded(&self, virtual_id: &str, source: &str) -> Option<Rc<BindResult>> {
        if let Some(cached) = self.cached_bind(virtual_id) {
            return Some(cached);
        }
        if let Some(cached) = super::cache::try_load_cache(self, virtual_id, source) {
            let bind_rc = Rc::new(cached.bind);
            self.store_bind(virtual_id.to_owned(), Rc::clone(&bind_rc));
            self.store_exports(
                ModuleId::stdlib(virtual_id).as_str().to_owned(),
                Rc::new(cached.exports),
            );
            return Some(bind_rc);
        }
        let (program, lex_errs) = self.parse_and_cache(source, virtual_id)?;
        Some(self.bind_and_cache(&program, lex_errs, virtual_id))
    }
}

impl ImportResolver for DiskResolver {
    fn module_bind(&self, abs_path: &str) -> Option<Rc<BindResult>> {
        if let Some(cached) = self.cached_bind(abs_path) {
            return Some(cached);
        }

        let canonical = varn_modules::canonical_or_original(Path::new(abs_path));
        let source = std::fs::read_to_string(&canonical).ok()?;

        if let Some(cached) = super::cache::try_load_cache(self, &canonical, &source) {
            let bind_rc = Rc::new(cached.bind);
            self.store_bind(canonical.clone(), Rc::clone(&bind_rc));
            self.store_exports(canonical, Rc::new(cached.exports));
            return Some(bind_rc);
        }

        let (program, lex_errs) = self.parse_and_cache(&source, &canonical)?;
        let bind = self.bind_and_cache(&program, lex_errs, &canonical);

        let base_dir = Path::new(&canonical).parent().unwrap_or(Path::new("."));
        let exports = self.collect(&program, bind.as_ref(), &canonical, base_dir, &mut Vec::new());
        super::cache::save_to_cache(self, &canonical, &source, &exports, bind.as_ref());

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

        match super::stdlib::stdlib_carrier(specifier)? {
            super::stdlib::Carrier::Blob => {
                self.from_interface_blob(specifier, &key).map(|(_, b)| b)
            }
            super::stdlib::Carrier::Embedded(source) => self.bind_from_embedded(specifier, source),
            super::stdlib::Carrier::File(abs) => self.module_bind(&abs),
        }
    }

    fn stdlib_exports(&self, specifier: &str) -> Rc<ExportMap> {
        let key = ModuleId::stdlib(specifier).as_str();
        if let Some(cached) = self.cached_exports(&key) {
            return cached;
        }

        let result = match super::stdlib::stdlib_carrier(specifier) {
            Some(super::stdlib::Carrier::Blob) => {
                self.from_interface_blob(specifier, &key).map(|(e, _)| e)
            }
            Some(super::stdlib::Carrier::Embedded(source)) => {
                Some(self.exports_from_embedded(specifier, source, &mut Vec::new()))
            }
            Some(super::stdlib::Carrier::File(abs)) => {
                Some(self.module_exports(&abs, &mut Vec::new()))
            }
            None => None,
        };

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
