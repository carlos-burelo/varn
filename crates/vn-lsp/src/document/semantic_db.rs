use rustc_hash::FxHashMap;

pub struct SemanticDB {
    pub expr_table: FxHashMap<vn_core::ast::AstId, vn_sem::output::TypeEntry>,

    pub expr_types: FxHashMap<u32, vn_sem::output::ExprInfo>,
    pub node_scopes: FxHashMap<u32, vn_sem::scope::ScopeId>,
    pub scope_spans: Vec<vn_sem::output::ScopeSpan>,

    pub symbol_types: FxHashMap<vn_sem::symbol::SymbolId, vn_sem::types::Type>,

    pub global_scope: vn_sem::scope::ScopeId,

    pub flattened_members: FxHashMap<String, Vec<vn_sem::types::ClassMemberInfo>>,

    pub member_resolutions: FxHashMap<u32, vn_sem::semantic_info::MemberResolution>,

    pub call_resolutions: FxHashMap<u32, vn_sem::semantic_info::CallResolution>,

    pub match_gaps: FxHashMap<vn_core::ast::AstId, vn_sem::semantic_info::MatchGap>,

    pub call_mappings: FxHashMap<vn_core::ast::AstId, Vec<Option<usize>>>,
    pub desugar: vn_sem::output::Desugarings,

    pub bind: vn_sem::bind::BindResult,

    pub types: std::cell::RefCell<std::sync::Arc<vn_sem::types::CheckerTyTable>>,
}

impl SemanticDB {
    pub fn name(&self, atom: vn_core::Atom) -> &str {
        self.bind.interner.resolve(atom)
    }

    pub fn resolve_at(
        &self,
        name: &str,
        cursor_offset: u32,
    ) -> Option<(vn_sem::symbol::SymbolId, vn_sem::types::Type)> {
        let scope_id = self.scope_at_offset(cursor_offset);
        let scope = self.bind.scopes.get(scope_id);
        let atom = self.bind.interner.get(name)?;
        let sym_id = scope.resolve(atom, &self.bind.scopes)?;
        let ty = self
            .symbol_types
            .get(&sym_id)
            .cloned()
            .or_else(|| self.bind.arena.get(sym_id).ty)
            .unwrap_or_default();
        Some((sym_id, ty))
    }

    pub fn scope_at_offset(&self, cursor_offset: u32) -> vn_sem::scope::ScopeId {
        let mut best_scope = self.global_scope;
        let mut best_span_len = u32::MAX;
        for span in &self.scope_spans {
            if cursor_offset >= span.start && cursor_offset <= span.end {
                let span_len = span.end.saturating_sub(span.start);
                if span_len < best_span_len {
                    best_span_len = span_len;
                    best_scope = span.scope;
                }
            }
        }
        best_scope
    }
}
