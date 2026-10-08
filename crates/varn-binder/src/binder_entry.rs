use super::Binder;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{AstArena, Program};
use varn_sem::bind::{Extensions, TypeMembers};
use varn_sem::scope::{CheckerScope, ScopeArena, ScopeKind};
use varn_sem::symbol::{Symbol, SymbolArena};

impl<'r> Binder<'r> {
    pub fn bind(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn varn_sem::resolver::ImportResolver,
    ) -> varn_sem::bind::BindResult {
        Self::bind_with_globals_iter(
            program,
            ast_arena,
            interner,
            resolver,
            None,
            FxHashMap::default(),
        )
    }

    pub fn bind_with_global_refs(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn varn_sem::resolver::ImportResolver,
        globals: &varn_sem::cores::CoreExports,
    ) -> varn_sem::bind::BindResult {
        Self::bind_with_globals_iter(
            program,
            ast_arena,
            interner,
            resolver,
            Some(&globals.table),
            globals
                .symbols
                .iter()
                .map(|(name, sym)| (name.clone(), sym.clone())),
        )
    }

    fn bind_with_globals_iter<I>(
        program: &Program,
        ast_arena: &'r AstArena,
        interner: varn_core::AtomInterner,
        resolver: &'r dyn varn_sem::resolver::ImportResolver,
        globals_table: Option<&varn_sem::types::CheckerTyTable>,
        globals: I,
    ) -> varn_sem::bind::BindResult
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
            deps: Vec::new(),
            annotation_types: FxHashMap::default(),
            match_arm_scopes: FxHashMap::default(),
            ty_table: Arc::new(varn_sem::types::CheckerTyTable::new()),
            source_file: Arc::from(program.filename.as_ref()),
            sum_type_variants: FxHashMap::default(),
            sum_variant_parent: FxHashMap::default(),
            sum_variant_fields: FxHashMap::default(),
            extensions: Extensions::default(),
            pending_enrich: Vec::new(),
            reported_type_forms: Default::default(),
            reported_params: Default::default(),
            array_watch: Vec::new(),
            evolved_array_types: FxHashMap::default(),
            type_decls: FxHashMap::default(),
            pending_decorator_roles: Vec::new(),
            user_decorators: rustc_hash::FxHashSet::default(),
        };

        let global = b.scopes.push(CheckerScope::new(ScopeKind::Global, None));
        b.current = global;
        let source_file = b.source_file.clone();
        b.intern_local(&source_file);
        if let Some(table) = globals_table {
            b.adopt(table);
        }

        for (name, sym) in globals {
            let name_atom = b.interner.intern(name.as_ref());
            let id = b.arena.push(sym);
            b.scopes.get_mut(global).define(name_atom, id);
        }

        b.bind_stmts(&program.body);
        b.finalize_array_watch(global);
        b.resolve_decorator_roles();
        Arc::make_mut(&mut b.ty_table).absorb_names(&b.interner);

        varn_sem::bind::BindResult {
            arena: b.arena,
            scopes: b.scopes,
            global_scope: global,
            diagnostics: b.diagnostics,
            interner: b.interner,
            deps: b.deps,
            annotation_types: b.annotation_types,
            match_arm_scopes: b.match_arm_scopes,
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
            user_decorators: b.user_decorators,
        }
    }
}
