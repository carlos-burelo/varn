use super::resolver_disk::DiskResolver;
use std::path::Path;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_sem::bind::BindResult;
use varn_sem::exports::ExportMap;

impl DiskResolver {
    pub(super) fn module_exports_uncached(
        &self,
        abs_path: &str,
        visiting: &mut Vec<String>,
    ) -> ExportMap {
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
        let Some(super::resolver_parse::ParsedModule {
            program,
            arena: ast_arena,
            interner,
            ..
        }) = self.parse_and_cache(&source, abs_path)
        else {
            return ExportMap::default();
        };
        let bind = self.cached_bind(abs_path).unwrap_or_else(|| {
            self.bind_and_cache(&program, ast_arena.as_ref(), interner, Vec::new(), abs_path)
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

    pub(super) fn exports_from_embedded(
        &self,
        virtual_id: &str,
        source: &varn_modules::loader::ModuleSource,
        visiting: &mut Vec<String>,
    ) -> Arc<ExportMap> {
        if visiting.iter().any(|v| v == virtual_id) {
            return Arc::new(ExportMap::default());
        }
        visiting.push(virtual_id.to_owned());

        if let Some(cached) = self.interface_of(virtual_id, source) {
            self.store_bind(virtual_id.to_owned(), Arc::new(cached.bind));
            visiting.pop();
            return Arc::new(cached.exports);
        }

        let Some(super::resolver_parse::ParsedModule {
            program,
            arena: ast_arena,
            interner,
            ..
        }) = self.parse_and_cache(&source.text, virtual_id)
        else {
            visiting.pop();
            return Arc::new(ExportMap::default());
        };
        let bind = self.bind_and_cache(
            &program,
            ast_arena.as_ref(),
            interner,
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

        super::cache::save_to_cache(
            self,
            virtual_id,
            &source.text,
            &exports,
            bind.as_ref(),
            super::CarrierKind::from(source.provenance.clone()),
        );
        visiting.pop();
        Arc::new(exports)
    }

    fn interface_of(
        &self,
        virtual_id: &str,
        source: &varn_modules::loader::ModuleSource,
    ) -> Option<super::cache::CachedModule> {
        match source.interface {
            Some(interface) => super::cache::adopt_interface(self, virtual_id, interface),
            None => super::cache::try_load_cache(
                self,
                virtual_id,
                &source.text,
                super::CarrierKind::from(source.provenance.clone()),
            ),
        }
    }

    pub(super) fn bind_from_embedded(
        &self,
        virtual_id: &str,
        source: &varn_modules::loader::ModuleSource,
    ) -> Option<Arc<BindResult>> {
        if let Some(cached) = self.cached_bind(virtual_id) {
            return Some(cached);
        }
        if let Some(cached) = self.interface_of(virtual_id, source) {
            let bind_rc = Arc::new(cached.bind);
            self.store_bind(virtual_id.to_owned(), Arc::clone(&bind_rc));
            self.store_exports(
                ModuleId::stdlib(virtual_id).as_str().to_owned(),
                Arc::new(cached.exports),
            );
            return Some(bind_rc);
        }
        let super::resolver_parse::ParsedModule {
            program,
            arena: ast_arena,
            lex_errs,
            interner,
        } = self.parse_and_cache(&source.text, virtual_id)?;
        Some(self.bind_and_cache(&program, ast_arena.as_ref(), interner, lex_errs, virtual_id))
    }
}
