use super::records::ExprInfo;
use super::Checker;
use crate::types::Type;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::AstArena;

impl<'r> Checker<'r> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        resolver: &'r dyn crate::module_resolver::ImportResolver,
        ast_arena: &'r AstArena,
        source_file: Arc<str>,
        global_scope: crate::scope::ScopeId,
        record_expr_types: bool,
        ty_table: Arc<crate::types::CheckerTyTable>,
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
            expr_types: FxHashMap::default(),
            infer_cache: FxHashMap::default(),
            infer_env_rev: 0,
            compat_cache: FxHashMap::default(),
            type_node_cache: FxHashMap::default(),
            symbol_type_params_cache: FxHashMap::default(),
            symbol_types: FxHashMap::default(),
            expr_table: FxHashMap::default(),
            expr_seq: 0,
            current_class: None,
            active_type_params: FxHashSet::default(),
            abstract_classes: FxHashSet::default(),
            is_assignment_target: false,
            in_pipeline_rhs: false,
            pipeline_value_type: None,
            desugar: super::records::Desugarings::default(),
            member_exists_cache: FxHashMap::default(),
            member_type_cache: FxHashMap::default(),
            expected_type: None,
            call_mappings: FxHashMap::default(),
            record_expr_types,
            node_scopes: FxHashMap::default(),
            scope_spans: Vec::new(),
            map_generics_cache: FxHashMap::default(),
            yielded_types: None,
            loop_depth: 0,
            switch_depth: 0,
            in_function: false,
            in_async: true,
            expected_object_members_cache: FxHashMap::default(),
            member_resolutions: FxHashMap::default(),
            call_resolutions: FxHashMap::default(),
            match_gaps: FxHashMap::default(),
            warned_deprecated: FxHashSet::default(),
            pure_scope: None,
            enclosing_caps: None,
            ty_table,
        }
    }

    pub(crate) fn project_expr_types(&mut self, bind: &crate::binder::BindResult) {
        let mut entries: Vec<(u32, u32, u32, ExprInfo)> = self
            .expr_table
            .values()
            .map(|e| {
                let span_len = e.end.saturating_sub(e.start);
                let info = ExprInfo {
                    ty: e.ty,
                    symbol_id: e.symbol_id,
                };
                (e.start, span_len, e.seq, info)
            })
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
        for (start, _len, _seq, info) in entries {
            self.expr_types.insert(start, info);
        }

        for (id, sym) in bind.arena.all().iter().enumerate() {
            if sym.origin_module.is_none() && sym.offset != 0 {
                self.expr_types
                    .entry(sym.offset)
                    .or_insert_with(|| ExprInfo {
                        ty: sym.ty.unwrap_or(Type::Dynamic),
                        symbol_id: Some(id),
                    });
            }
        }
    }
}
