use super::resolver_disk::DiskResolver;
use super::ExportMap;
use crate::binder::BindResult;
use std::path::Path;
use std::sync::Arc;

pub(super) struct ParsedModule {
    pub(super) program: Arc<varn_core::ast::Program>,
    pub(super) arena: Arc<varn_core::ast::AstArena>,
    pub(super) lex_errs: Vec<varn_core::Diagnostic>,
    pub(super) interner: varn_core::AtomInterner,
}

impl DiskResolver {
    pub(super) fn parse_and_cache(&self, source: &str, key: &str) -> Option<ParsedModule> {
        let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, key);
        let (program, interner, arena) =
            varn_parser::parse(tokens, lexeme_buf, key, varn_core::AtomInterner::new()).ok()?;
        let program = Arc::new(program);
        let arena = Arc::new(arena);
        self.store_program(key.to_owned(), Arc::clone(&program));
        self.store_arena(key.to_owned(), Arc::clone(&arena));
        Some(ParsedModule {
            program,
            arena,
            lex_errs,
            interner,
        })
    }

    pub(super) fn is_binding(&self, key: &str) -> bool {
        self.in_flight.lock().contains(key)
    }

    pub(super) fn begin_cache_load(&self, key: &str) -> bool {
        self.in_flight.lock().insert(key.to_owned())
    }

    pub(super) fn end_cache_load(&self, key: &str) {
        self.in_flight.lock().remove(key);
    }

    pub(super) fn bind_and_cache(
        &self,
        program: &varn_core::ast::Program,
        ast_arena: &varn_core::ast::AstArena,
        interner: varn_core::AtomInterner,
        lex_errs: Vec<varn_core::Diagnostic>,
        key: &str,
    ) -> Arc<BindResult> {
        self.in_flight.lock().insert(key.to_owned());
        let mut bind = match crate::core::loader::module_globals(key, self) {
            Some(globals) => crate::binder::Binder::bind_with_global_refs(
                program, ast_arena, interner, self, &globals,
            ),
            None => crate::binder::Binder::bind(program, ast_arena, interner, self),
        };
        self.in_flight.lock().remove(key);
        for e in lex_errs {
            bind.diagnostics.emit(e);
        }
        let bind = Arc::new(bind);
        self.store_bind(key.to_owned(), Arc::clone(&bind));
        bind
    }

    pub(super) fn collect(
        &self,
        program: &varn_core::ast::Program,
        ast_arena: &varn_core::ast::AstArena,
        bind: &BindResult,
        key: &str,
        base_dir: &Path,
        visiting: &mut Vec<String>,
    ) -> ExportMap {
        let mut exports = ExportMap::with_table(bind.ty_table.clone());
        super::exports::collect_exports(
            self,
            &program.body,
            ast_arena,
            bind,
            key,
            base_dir,
            visiting,
            &mut exports,
        );
        super::exports::assign_slots(&mut exports);
        exports
    }
}
