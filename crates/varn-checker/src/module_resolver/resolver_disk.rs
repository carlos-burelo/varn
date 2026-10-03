use super::graph::ModuleGraph;
use super::resolver_api::CoreExportsMap;
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
    pub(super) core_exports: parking_lot::Mutex<Option<Arc<CoreExportsMap>>>,
    pub(super) core_members: parking_lot::Mutex<Option<Arc<crate::core::loader::CoreMembers>>>,
    /// The single `Atom` table for this compilation. Every `varn_parser::parse`
    /// this resolver drives (the entry file included, via
    /// `interner_snapshot`/`set_interner`) reads from and grows this same
    /// table, so an `Atom` minted while parsing one module compares equal to
    /// the same text minted while parsing another — see `Symbol::origin_module`.
    /// A resolver-per-file `AtomInterner` was the bug: two parses never shared
    /// one, so their `Atom` indices meant nothing to each other.
    pub(super) interner: parking_lot::Mutex<varn_core::AtomInterner>,
    /// The single `CheckerTyId` table for this compilation — same reasoning
    /// and lifecycle as `interner` above (see `ImportResolver::ty_table_snapshot`).
    pub(super) ty_table: parking_lot::Mutex<std::sync::Arc<crate::types::CheckerTyTable>>,
}

impl Default for DiskResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// The shared `Atom` table starts with the builtin type names: the checker
/// synthesizes types such as `Range<char>` for source text that never spells
/// the name, and every module's snapshot must already resolve that atom.
fn seeded_interner() -> varn_core::AtomInterner {
    let mut interner = varn_core::AtomInterner::default();
    for builtin in varn_core::BuiltinType::ALL {
        interner.intern(builtin.name());
    }
    interner
}

impl DiskResolver {
    pub fn new() -> Self {
        Self {
            loader: varn_modules::loader::default_registry(),
            graph: parking_lot::Mutex::default(),
            in_flight: parking_lot::Mutex::default(),
            core_exports: parking_lot::Mutex::default(),
            core_members: parking_lot::Mutex::default(),
            interner: parking_lot::Mutex::new(seeded_interner()),
            ty_table: parking_lot::Mutex::default(),
        }
    }

    /// The one way this resolver obtains a module's source or precomputed
    /// artifacts.
    pub(super) fn load_source(&self, id: &ModuleId) -> Option<varn_modules::loader::ModuleSource> {
        use varn_modules::loader::ModuleLoader;
        self.loader.source(id).ok()
    }

    /// A clone of the compilation's `Atom` table as of now. Cheap relative to
    /// a parse, and the only way to hand modules-so-far's interned text to a
    /// caller without exposing the `RefCell` itself: `AtomInterner::clone`
    /// copies the dedup map and string vec, but every `Atom` it already
    /// contains keeps the same index, so an atom resolved through this clone
    /// resolves identically through the resolver's live table or through any
    /// other snapshot taken later.
    pub fn interner_snapshot(&self) -> varn_core::AtomInterner {
        self.interner.lock().clone()
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
    pub fn set_interner(&self, interner: &varn_core::AtomInterner) {
        let mut live = self.interner.lock();
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
