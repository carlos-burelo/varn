use super::graph::ModuleGraph;
use std::sync::Arc;
use varn_core::ModuleId;

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
    pub(super) loader: varn_modules::loader::ModuleRegistry,
    pub(super) graph: parking_lot::Mutex<ModuleGraph>,
    /// Modules whose binding is currently on the stack. Used to break
    /// mutual-import deadlocks when a module's body imports a peer that then
    /// asks for `core:types` again to expand a generic alias.
    pub(super) in_flight: parking_lot::Mutex<rustc_hash::FxHashSet<String>>,
    /// The prelude, derived once from the stdlib this resolver serves.
    ///
    /// Lives here rather than in a process-wide static because it is a
    /// *function of the stdlib in use*: a process that switches std provenance
    /// (the language server does, between the checkout tree and the embedded
    /// bundle) would otherwise keep answering from the first one it ever saw.
    pub(super) core_exports: parking_lot::Mutex<Option<Arc<crate::core::loader::CoreExports>>>,
    pub(super) core_members: parking_lot::Mutex<Option<Arc<crate::core::loader::CoreMembers>>>,
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
            graph: parking_lot::Mutex::default(),
            in_flight: parking_lot::Mutex::default(),
            core_exports: parking_lot::Mutex::default(),
            core_members: parking_lot::Mutex::default(),
        }
    }

    /// The one way this resolver obtains a module's source or precomputed
    /// artifacts.
    pub(super) fn load_source(&self, id: &ModuleId) -> Option<varn_modules::loader::ModuleSource> {
        use varn_modules::loader::ModuleLoader;
        self.loader.source(id).ok()
    }

    /// Evict `id` and everything that transitively imports it.
    pub fn invalidate(&self, id: &ModuleId) {
        self.graph.lock().invalidate(id);
    }

    /// Drop every memoized module, the prelude included: a std swap invalidates
    /// it just as surely as an edit invalidates a workspace module.
    pub fn clear(&self) {
        self.graph.lock().clear();
        *self.core_exports.lock() = None;
        *self.core_members.lock() = None;
    }

    pub fn types_cache_dir(&self) -> std::path::PathBuf {
        // Clone the root and drop the borrow before calling out. Resolution is
        // re-entrant, and a borrow spanning a call into another crate is the
        // kind of thing that only fails once someone makes that crate call
        // back.
        let root = self.graph.lock().project_root_or_init().clone();
        varn_modules::artifact::get_types_cache_dir(&root)
    }
}
