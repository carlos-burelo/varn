use super::resolver_disk::DiskResolver;
use std::path::Path;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_sem::bind::BindResult;
use varn_sem::exports::ExportMap;
use varn_sem::resolver::ImportResolver;

impl ImportResolver for DiskResolver {
    fn evict_heavy(&self) -> (usize, usize, usize) {
        self.graph.lock().evict_heavy()
    }

    fn graph_stats(&self) -> (usize, usize, usize, usize) {
        self.graph.lock().heavy_stats()
    }

    fn module_bind(&self, abs_path: &str) -> Option<Arc<BindResult>> {
        if let Some(cached) = self.cached_bind(abs_path) {
            return Some(cached);
        }

        let canonical = varn_modules::canonical_or_original(Path::new(abs_path));

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

        let super::resolver_parse::ParsedModule {
            program,
            arena: ast_arena,
            lex_errs,
            interner,
        } = self.parse_and_cache(source, &canonical)?;
        let bind =
            self.bind_and_cache(&program, ast_arena.as_ref(), interner, lex_errs, &canonical);

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

    fn core_exports(&self) -> Arc<varn_sem::cores::CoreExports> {
        if let Some(hit) = self.core_exports.lock().as_ref() {
            return Arc::clone(hit);
        }

        let built = Arc::new(varn_binder::core::loader::build_core_exports(self));
        *self.core_exports.lock() = Some(Arc::clone(&built));
        built
    }

    fn core_members(&self) -> Arc<varn_sem::cores::CoreMembers> {
        if let Some(hit) = self.core_members.lock().as_ref() {
            return Arc::clone(hit);
        }
        let built = Arc::new(varn_binder::core::loader::build_core_members(self));
        *self.core_members.lock() = Some(Arc::clone(&built));
        built
    }
}
