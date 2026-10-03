use super::resolver_api::ImportResolver;
use super::resolver_disk::DiskResolver;
use super::ExportMap;
use crate::binder::BindResult;
use std::path::Path;
use std::sync::Arc;

impl DiskResolver {
    pub(super) fn parse_and_cache(
        &self,
        source: &str,
        key: &str,
    ) -> Option<(
        Arc<varn_core::ast::Program>,
        Arc<varn_core::ast::AstArena>,
        Vec<varn_core::Diagnostic>,
    )> {
        let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, key);
        // Seed this parse from a clone of the shared table rather than handing
        // it out by value: on a parse error the clone is simply dropped and
        // the resolver's own table is untouched, so a module that fails to
        // parse never rolls back atoms other modules already minted.
        let interner = self.interner_snapshot();
        let (program, interner, arena) =
            varn_parser::parse(tokens, lexeme_buf, key, interner).ok()?;
        self.set_interner(&interner);
        let program = Arc::new(program);
        let arena = Arc::new(arena);
        self.store_program(key.to_owned(), Arc::clone(&program));
        self.store_arena(key.to_owned(), Arc::clone(&arena));
        Some((program, arena, lex_errs))
    }

    /// True while `key`'s bind is in progress; see [`DiskResolver::in_flight`].
    pub(super) fn is_binding(&self, key: &str) -> bool {
        self.in_flight.lock().contains(key)
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
            // Building the core globals may have minted atoms `interner`
            // never saw; the live snapshot is a superset of it.
            Some(globals) => crate::binder::Binder::bind_with_global_refs(
                program,
                ast_arena,
                self.interner_snapshot(),
                self,
                &globals,
            ),
            None => crate::binder::Binder::bind(program, ast_arena, interner, self),
        };
        self.in_flight.lock().remove(key);
        for e in lex_errs {
            bind.diagnostics.emit(e);
        }
        // Binding itself coins new atoms (doc comments, "constructor", "this",
        // mangled extension names, ...) on top of whatever parsing produced.
        // Without publishing them back, a later `interner_snapshot()` (e.g.
        // `save_to_cache`) resolves against a table that never saw them and
        // panics out of bounds — same fix as `parse_and_cache`.
        self.set_interner(&bind.interner);
        self.set_ty_table(bind.ty_table.clone());
        let bind = Arc::new(bind);
        self.store_bind(key.to_owned(), Arc::clone(&bind));
        bind
    }

    /// Collect a program's exports, resolving its own imports through `self`.
    pub(super) fn collect(
        &self,
        program: &varn_core::ast::Program,
        ast_arena: &varn_core::ast::AstArena,
        bind: &BindResult,
        key: &str,
        base_dir: &Path,
        visiting: &mut Vec<String>,
    ) -> ExportMap {
        let mut exports = ExportMap::default();
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
