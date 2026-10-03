use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use std::path::Path;
use std::sync::Arc;

pub(super) type CoreExportsMap = rustc_hash::FxHashMap<Arc<str>, crate::symbol::Symbol>;
pub trait ImportResolver {
    /// Bind a workspace module identified by canonical absolute path.
    fn module_bind(&self, abs_path: &str) -> Option<Arc<BindResult>>;

    /// Exports of a workspace module. `visiting` carries the in-progress cycle
    /// set; import cycles resolve to an empty map rather than recursing.
    fn module_exports(&self, abs_path: &str, visiting: &mut Vec<String>) -> Arc<ExportMap>;

    /// Bind a `std:` / `core:` / `runtime:` module.
    fn stdlib_bind(&self, specifier: &str) -> Option<Arc<BindResult>>;

    /// Exports of a `std:` / `core:` / `runtime:` module.
    fn stdlib_exports(&self, specifier: &str) -> Arc<ExportMap>;

    /// Turn an import specifier into an absolute path, relative to `base_dir`.
    fn resolve_specifier(&self, base_dir: &Path, specifier: &str) -> Option<String>;

    /// Note that `importer` depends on `imported`, so invalidating the latter
    /// can evict the former.
    fn record_dep(&self, importer: &str, imported: &str);

    /// The prelude's global symbols (`core:*`), as this resolver's stdlib
    /// defines them. Part of the trait because which prelude is in force is a
    /// property of which stdlib you resolve against.
    fn core_exports(&self) -> Arc<rustc_hash::FxHashMap<Arc<str>, crate::symbol::Symbol>>;

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

    /// Number of shapes interned in this resolver's live `CheckerTyId` table,
    /// without cloning it (unlike [`Self::ty_table_snapshot`]) — memory
    /// introspection reads this on every open document; cloning the table to
    /// answer "how big is it" would be the exact bug it is trying to surface.
    fn ty_table_len(&self) -> usize;

    /// Evict heavy memoized artifacts (`binds`, parsed `programs`, AST
    /// `arenas`) while keeping `exports`, specifier paths and dependency
    /// edges.
    ///
    /// Every evicted entry re-derives from source (or the on-disk artifact
    /// cache) on next touch — `module_bind` and `module_exports` both rebuild
    /// on miss — so this changes peak memory, not answers. Returns per-table
    /// evicted counts `(binds, programs, arenas)` for observability.
    fn evict_heavy(&self) -> (usize, usize, usize);

    /// Counts of `(binds, programs, arenas, exports)` memoized right now.
    /// Observability for [`Self::evict_heavy`]; the LSP `memoryStats` command
    /// reports it per process.
    fn graph_stats(&self) -> (usize, usize, usize, usize);

    /// The prelude's member tables.
    fn core_members(&self) -> Arc<crate::core::loader::CoreMembers>;

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
    ) -> Option<Arc<BindResult>> {
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
