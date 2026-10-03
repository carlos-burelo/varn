use crate::binder::BindResult;
use crate::module_resolver::cache::ExportMap;
use std::path::Path;
use std::sync::Arc;

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
    fn core_exports(&self) -> Arc<crate::core::loader::CoreExports>;

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
