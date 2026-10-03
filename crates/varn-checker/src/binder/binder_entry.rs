use super::Binder;
use crate::binder::{Extensions, TypeMembers};
use crate::scope::{CheckerScope, ScopeArena, ScopeKind};
use crate::symbol::{Symbol, SymbolArena};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, Program};

impl<'r> Binder<'r> {
    pub fn bind(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn crate::module_resolver::ImportResolver,
    ) -> super::BindResult {
        Self::bind_with_globals_iter(program, ast_arena, interner, resolver, FxHashMap::default())
    }

    pub fn bind_with_global_refs(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn crate::module_resolver::ImportResolver,
        globals: &FxHashMap<Arc<str>, Symbol>,
    ) -> super::BindResult {
        Self::bind_with_globals_iter(
            program,
            ast_arena,
            interner,
            resolver,
            globals
                .iter()
                .map(|(name, sym)| (name.clone(), sym.clone())),
        )
    }

    fn bind_with_globals_iter<I>(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn crate::module_resolver::ImportResolver,
        globals: I,
    ) -> super::BindResult
    where
        I: IntoIterator<Item = (Arc<str>, Symbol)>,
    {
        let mut b = Binder {
            resolver,
            ast_arena,
            arena: SymbolArena::default(),
            scopes: ScopeArena::default(),
            current: 0,
            class_methods: FxHashMap::default(),
            type_members: TypeMembers::default(),
            class_parents: FxHashMap::default(),
            diagnostics: varn_core::DiagnosticBag::new(),
            interner,
            ty_table: resolver.ty_table_snapshot(),
            source_file: Arc::from(program.filename.as_ref()),
            sum_type_variants: FxHashMap::default(),
            sum_variant_parent: FxHashMap::default(),
            sum_variant_fields: FxHashMap::default(),
            extensions: Extensions::default(),
            pending_enrich: Vec::new(),
            reported_type_forms: Default::default(),
            array_watch: Vec::new(),
            evolved_array_types: FxHashMap::default(),
            type_decls: FxHashMap::default(),
        };

        let global = b.scopes.push(CheckerScope::new(ScopeKind::Global, None));
        b.current = global;

        for (name, sym) in globals {
            let name_atom = b.interner.intern(name.as_ref());
            let id = b.arena.push(sym);
            b.scopes.get_mut(global).define(name_atom, id);
        }

        b.bind_stmts(&program.body);
        b.finalize_array_watch(global);

        super::BindResult {
            arena: b.arena,
            scopes: b.scopes,
            global_scope: global,
            diagnostics: b.diagnostics,
            interner: b.interner,
            ty_table: b.ty_table,
            class_methods: b.class_methods,
            type_members: b.type_members,
            class_parents: b.class_parents,
            source_file: b.source_file,
            sum_type_variants: b.sum_type_variants,
            sum_variant_parent: b.sum_variant_parent,
            sum_variant_fields: b.sum_variant_fields,
            extensions: b.extensions,
            core: None,
            pending_enrich: b.pending_enrich,
            evolved_array_types: b.evolved_array_types,
        }
    }
}
