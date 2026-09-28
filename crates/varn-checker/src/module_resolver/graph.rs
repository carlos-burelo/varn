use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;
use varn_core::ModuleId;

/// Everything the checker memoizes about *other* modules, in one owned place.
///
/// This used to be six separate `thread_local!` statics. Splitting one piece of
/// state across six anonymous globals is what let the invalidation bug hide:
/// `invalidate_module_cache` cleared whichever thread happened to call it, so a
/// language server running analysis on a pool of blocking workers kept stale
/// binds on every other worker, and cross-module resolution came out right or
/// wrong depending on who answered the request.
///
/// Naming the state and giving it an owner is the precondition for handing that
/// ownership to the caller (see `ImportResolver` in `docs/LSP_ARCHITECTURE.md`);
/// a checker that owns no cache has nothing to go stale.
///
/// Not to be confused with the two *immutable* memo caches elsewhere in this
/// crate (`core::loader`, `binder::type_resolution::aliases`): those derive from
/// the standard library, are loaded once, and are never invalidated. They are a
/// different problem — they block `Send`, but they cannot go stale.
#[derive(Default)]
pub struct ModuleGraph {
    /// Bound modules, keyed by canonical absolute path (or `std:`-style id).
    binds: FxHashMap<String, Arc<BindResult>>,
    exports: FxHashMap<String, Arc<ExportMap>>,
    programs: FxHashMap<String, Arc<varn_core::ast::Program>>,
    /// The `AstArena` each cached `program` was parsed into — same key, same
    /// lifetime, stored alongside it since an `ExprId`/`StmtId` in `program`
    /// resolves only against the arena it was allocated from.
    arenas: FxHashMap<String, Arc<varn_core::ast::AstArena>>,
    /// `(base_dir, specifier)` → resolved absolute path.
    resolved_paths: FxHashMap<(String, String), String>,
    /// imported module → modules that import it. Drives transitive eviction.
    reverse_deps: FxHashMap<String, Vec<String>>,
    project_root: Option<PathBuf>,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self::default()
    }

    // ── binds ────────────────────────────────────────────────────────────

    pub fn bind(&self, key: &str) -> Option<Arc<BindResult>> {
        self.binds.get(key).map(Arc::clone)
    }

    /// First write wins: a module already bound in this graph must keep its
    /// identity, or callers holding an `Rc` to the old one would silently
    /// disagree with callers that fetch it later.
    pub fn insert_bind(&mut self, key: String, bind: Arc<BindResult>) {
        self.binds.entry(key).or_insert(bind);
    }

    // ── exports ──────────────────────────────────────────────────────────

    pub fn exports(&self, key: &str) -> Option<Arc<ExportMap>> {
        self.exports.get(key).map(Arc::clone)
    }

    pub fn insert_exports(&mut self, key: String, exports: Arc<ExportMap>) {
        self.exports.insert(key, exports);
    }

    // ── parsed programs ──────────────────────────────────────────────────

    pub fn program(&self, key: &str) -> Option<Arc<varn_core::ast::Program>> {
        self.programs.get(key).map(Arc::clone)
    }

    pub fn insert_program(&mut self, key: String, program: Arc<varn_core::ast::Program>) {
        self.programs.entry(key).or_insert(program);
    }

    // ── AST arenas ───────────────────────────────────────────────────────

    pub fn arena(&self, key: &str) -> Option<Arc<varn_core::ast::AstArena>> {
        self.arenas.get(key).map(Arc::clone)
    }

    pub fn insert_arena(&mut self, key: String, arena: Arc<varn_core::ast::AstArena>) {
        self.arenas.entry(key).or_insert(arena);
    }

    // ── specifier resolution ─────────────────────────────────────────────

    pub fn resolved_path(&self, base_dir: &str, specifier: &str) -> Option<String> {
        self.resolved_paths
            .get(&(base_dir.to_owned(), specifier.to_owned()))
            .cloned()
    }

    pub fn insert_resolved_path(&mut self, base_dir: String, specifier: String, abs: String) {
        self.resolved_paths.insert((base_dir, specifier), abs);
    }

    // ── dependency graph ─────────────────────────────────────────────────

    pub fn record_dep(&mut self, importer: &str, imported: &str) {
        self.reverse_deps
            .entry(imported.to_owned())
            .or_default()
            .push(importer.to_owned());
    }

    // ── project root ─────────────────────────────────────────────────────

    pub fn project_root_or_init(&mut self) -> &PathBuf {
        self.project_root.get_or_insert_with(|| {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            varn_modules::artifact::find_project_root(&cwd)
        })
    }

    // ── invalidation ─────────────────────────────────────────────────────

    /// Evict `id` and everything that transitively imports it.
    pub fn invalidate(&mut self, id: &ModuleId) {
        let key = id.as_str();
        let mut to_clear = Vec::new();
        let mut visited = rustc_hash::FxHashSet::default();
        let mut queue = vec![key.clone()];

        while let Some(k) = queue.pop() {
            if !visited.insert(k.clone()) {
                continue;
            }
            if let Some(deps) = self.reverse_deps.get(&k) {
                queue.extend(deps.iter().cloned());
            }
            to_clear.push(k);
        }

        for k in &to_clear {
            self.binds.remove(k);
            self.exports.remove(k);
            self.programs.remove(k);
            self.arenas.remove(k);
        }
        self.resolved_paths.retain(|_, v| !to_clear.contains(v));
    }

    /// Drop binds, parsed programs and AST arenas, keeping exports,
    /// specifier paths, dependency edges and project root.
    ///
    /// The evicted tables are pure memoization: every reader re-derives on
    /// miss (`module_bind` re-parses + re-binds, `module_exports` re-collects,
    /// both consult the on-disk artifact cache first). What stays is exactly
    /// what answering later queries needs without re-reading: the export maps
    /// (small, portable) and the graph shape that invalidation walks.
    /// Returns per-table evicted counts `(binds, programs, arenas)`.
    pub fn evict_heavy(&mut self) -> (usize, usize, usize) {
        let counts = (self.binds.len(), self.programs.len(), self.arenas.len());
        self.binds.clear();
        self.programs.clear();
        self.arenas.clear();
        counts
    }

    /// Counts of `(binds, programs, arenas, exports)` memoized right now.
    pub fn heavy_stats(&self) -> (usize, usize, usize, usize) {
        (
            self.binds.len(),
            self.programs.len(),
            self.arenas.len(),
            self.exports.len(),
        )
    }

    /// Drop every memoized module. `project_root` survives: it describes where
    /// the workspace is, not what is in it.
    pub fn clear(&mut self) {
        self.binds.clear();
        self.exports.clear();
        self.programs.clear();
        self.arenas.clear();
        self.resolved_paths.clear();
        self.reverse_deps.clear();
    }
}

#[cfg(test)]
mod tests {
    /// The graph caches `BindResult`/`ExportMap`/`Program`/`AstArena`. With the
    /// last `Rc` (this file) moved to `Arc`, the cache itself must be
    /// `Send + Sync` so it can back a resolver shared across checker workers
    /// (Ley 3, ADR-0012). A reintroduced `Rc` here fails the suite.
    #[test]
    fn module_graph_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<super::ModuleGraph>();
    }
}
