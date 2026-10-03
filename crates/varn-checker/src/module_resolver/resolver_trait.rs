use super::resolver_api::ImportResolver;
use super::resolver_disk::DiskResolver;
use super::ExportMap;
use crate::binder::BindResult;
use std::path::Path;
use std::sync::Arc;
use varn_core::ModuleId;

impl ImportResolver for DiskResolver {
    fn interner_snapshot(&self) -> varn_core::AtomInterner {
        self.interner.lock().clone()
    }

    fn ty_table_snapshot(&self) -> std::sync::Arc<crate::types::CheckerTyTable> {
        self.ty_table.lock().clone()
    }

    fn set_ty_table(&self, table: std::sync::Arc<crate::types::CheckerTyTable>) {
        // Merge, never replace: `table` is one module's locally-grown view,
        // which can disagree with the live table past their common prefix.
        // `absorb` keeps live's own indices stable and only learns shapes it
        // is missing.
        let mut live = self.ty_table.lock();
        std::sync::Arc::make_mut(&mut live).absorb(&table);
    }

    fn intern_ty(&self, kind: crate::types::InternedTypeKind) -> crate::types::CheckerTyId {
        let mut live = self.ty_table.lock();
        std::sync::Arc::make_mut(&mut live).intern(kind)
    }

    fn interner_len(&self) -> usize {
        self.interner.lock().len()
    }

    fn ty_table_len(&self) -> usize {
        self.ty_table.lock().len()
    }

    fn evict_heavy(&self) -> (usize, usize, usize) {
        self.graph.lock().evict_heavy()
    }

    fn graph_stats(&self) -> (usize, usize, usize, usize) {
        self.graph.lock().heavy_stats()
    }

    fn intern(&self, s: &str) -> varn_core::Atom {
        self.interner.lock().intern(s)
    }

    fn module_bind(&self, abs_path: &str) -> Option<Arc<BindResult>> {
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

        let source = self.load_source(&ModuleId::local_str(&canonical))?;
        let carrier = super::CarrierKind::from(source.provenance);
        let source = source.text;
        let source = source.as_ref();

        if let Some(cached) = super::cache::try_load_cache(self, &canonical, source, carrier) {
            let bind_rc = Arc::new(cached.bind);
            self.store_bind(canonical.clone(), Arc::clone(&bind_rc));
            self.store_exports(canonical, Arc::new(cached.exports));
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

    fn module_exports(&self, abs_path: &str, visiting: &mut Vec<String>) -> Arc<ExportMap> {
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
            return Arc::new(ExportMap::default());
        }

        // Publish an empty map before recursing: a cycle that reaches this
        // module again finds the sentinel instead of recursing forever.
        self.store_exports(canonical.clone(), Arc::new(ExportMap::default()));

        visiting.push(canonical.clone());
        let result = Arc::new(self.module_exports_uncached(&canonical, visiting));
        visiting.pop();

        self.store_exports(canonical, Arc::clone(&result));
        result
    }

    fn stdlib_bind(&self, specifier: &str) -> Option<Arc<BindResult>> {
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

    fn stdlib_exports(&self, specifier: &str) -> Arc<ExportMap> {
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
                self.store_exports(key, Arc::clone(&exports));
                exports
            }
            None => Arc::new(ExportMap::default()),
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
        self.graph.lock().record_dep(importer, imported);
    }

    fn core_exports(&self) -> Arc<rustc_hash::FxHashMap<Arc<str>, crate::symbol::Symbol>> {
        if let Some(hit) = self.core_exports.lock().as_ref() {
            return Arc::clone(hit);
        }
        // Built with the borrow released: building resolves stdlib modules
        // through `self`, which takes the same borrows.
        let built = Arc::new(crate::core::loader::build_core_exports(self));
        *self.core_exports.lock() = Some(Arc::clone(&built));
        built
    }

    fn core_members(&self) -> Arc<crate::core::loader::CoreMembers> {
        if let Some(hit) = self.core_members.lock().as_ref() {
            return Arc::clone(hit);
        }
        let built = Arc::new(crate::core::loader::build_core_members(self));
        *self.core_members.lock() = Some(Arc::clone(&built));
        built
    }
}
