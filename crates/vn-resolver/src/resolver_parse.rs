use super::module_resolver::ModuleResolver;
use std::path::Path;
use std::sync::Arc;
use vn_sem::bind::BindResult;
use vn_sem::exports::ExportMap;

pub(super) struct ParsedModule {
    pub(super) program: Arc<vn_core::ast::Program>,
    pub(super) arena: Arc<vn_core::ast::AstArena>,
    pub(super) lex_errs: Vec<vn_core::Diagnostic>,
    pub(super) interner: vn_core::AtomInterner,
}

impl ModuleResolver {
    pub(super) fn parse_and_cache(&self, source: &str, key: &str) -> Option<ParsedModule> {
        let (tokens, lexeme_buf, lex_errs) = vn_lexer::scan(source, key);
        let (program, interner, arena) =
            match vn_parser::parse(tokens, lexeme_buf, key, vn_core::AtomInterner::new()) {
                Ok(parsed) => parsed,
                Err(errors) => {
                    let diagnostics: Vec<vn_core::Diagnostic> =
                        lex_errs.iter().cloned().chain(errors).collect();
                    self.syntax_failures.lock().insert(
                        key.to_owned(),
                        Arc::new(vn_sem::resolver::SyntaxFailure {
                            source: Arc::from(source),
                            diagnostics,
                        }),
                    );
                    return None;
                }
            };
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
        program: &vn_core::ast::Program,
        ast_arena: &vn_core::ast::AstArena,
        interner: vn_core::AtomInterner,
        lex_errs: Vec<vn_core::Diagnostic>,
        key: &str,
    ) -> Arc<BindResult> {
        self.in_flight.lock().insert(key.to_owned());
        let mut bind = match vn_binder::core::loader::module_globals(key, self) {
            Some(globals) => vn_binder::Binder::bind_with_global_refs(
                program,
                ast_arena,
                interner.clone(),
                self,
                &globals,
            ),
            None => vn_binder::Binder::bind(program, ast_arena, interner.clone(), self),
        };
        for e in lex_errs {
            bind.diagnostics.push(e);
        }
        let provisional = Arc::new(bind);
        self.store_bind(key.to_owned(), Arc::clone(&provisional));
        let mut checked = vn_checker::Checker::check(program, ast_arena, interner, self).bind;
        checked.diagnostics = provisional.diagnostics.clone();
        self.in_flight.lock().remove(key);
        let checked = Arc::new(checked);
        self.replace_bind(key.to_owned(), Arc::clone(&checked));
        checked
    }

    pub(super) fn collect(
        &self,
        program: &vn_core::ast::Program,
        ast_arena: &vn_core::ast::AstArena,
        bind: &BindResult,
        source: &str,
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
            source,
            key,
            base_dir,
            visiting,
            &mut exports,
        );
        super::exports::assign_slots(&mut exports);
        exports
    }
}
