use super::Checker;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::AstArena;

impl<'r> Checker<'r> {
    pub(crate) fn new(
        resolver: &'r dyn varn_sem::resolver::ImportResolver,
        ast_arena: &'r AstArena,
        source_file: Arc<str>,
        global_scope: varn_sem::scope::ScopeId,
        ty_table: Arc<varn_sem::types::CheckerTyTable>,
    ) -> Self {
        Checker {
            resolver,
            ast_arena,
            current_scope: global_scope,
            diagnostics: varn_core::DiagnosticBag::new(),
            source_file,
            expected_return_type: None,
            narrowed_types: FxHashMap::default(),
            narrowings_cache: FxHashMap::default(),
            child_indices: FxHashMap::default(),
            infer_cache: FxHashMap::default(),
            infer_env_rev: 0,
            compat_cache: FxHashMap::default(),
            type_node_cache: FxHashMap::default(),
            symbol_type_params_cache: FxHashMap::default(),
            current_class: None,
            active_type_params: FxHashSet::default(),
            abstract_classes: FxHashSet::default(),
            is_assignment_target: false,
            in_pipeline_rhs: false,
            pipeline_value_type: None,
            member_exists_cache: FxHashMap::default(),
            member_type_cache: FxHashMap::default(),
            expected_type: None,
            map_generics_cache: FxHashMap::default(),
            yielded_types: None,
            loop_depth: 0,
            switch_depth: 0,
            in_function: false,
            in_async: true,
            expected_object_members_cache: FxHashMap::default(),
            warned_deprecated: FxHashSet::default(),
            pure_scope: None,
            enclosing_caps: None,
            ty_table,
        }
    }
}
