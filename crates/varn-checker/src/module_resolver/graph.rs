use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::rc::Rc;
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
    binds: FxHashMap<String, Rc<BindResult>>,
    exports: FxHashMap<String, Rc<ExportMap>>,
    programs: FxHashMap<String, Rc<varn_core::ast::Program>>,
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

    pub fn bind(&self, key: &str) -> Option<Rc<BindResult>> {
        self.binds.get(key).map(Rc::clone)
    }

    /// First write wins: a module already bound in this graph must keep its
    /// identity, or callers holding an `Rc` to the old one would silently
    /// disagree with callers that fetch it later.
    pub fn insert_bind(&mut self, key: String, bind: Rc<BindResult>) {
        self.binds.entry(key).or_insert(bind);
    }

    // ── exports ──────────────────────────────────────────────────────────

    pub fn exports(&self, key: &str) -> Option<Rc<ExportMap>> {
        self.exports.get(key).map(Rc::clone)
    }

    pub fn insert_exports(&mut self, key: String, exports: Rc<ExportMap>) {
        self.exports.insert(key, exports);
    }

    // ── parsed programs ──────────────────────────────────────────────────

    pub fn program(&self, key: &str) -> Option<Rc<varn_core::ast::Program>> {
        self.programs.get(key).map(Rc::clone)
    }

    pub fn insert_program(&mut self, key: String, program: Rc<varn_core::ast::Program>) {
        self.programs.entry(key).or_insert(program);
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
        let mut visited = std::collections::HashSet::new();
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
        }
        self.resolved_paths.retain(|_, v| !to_clear.contains(v));
    }

    /// Drop every memoized module. `project_root` survives: it describes where
    /// the workspace is, not what is in it.
    pub fn clear(&mut self) {
        self.binds.clear();
        self.exports.clear();
        self.programs.clear();
        self.resolved_paths.clear();
        self.reverse_deps.clear();
    }
}
