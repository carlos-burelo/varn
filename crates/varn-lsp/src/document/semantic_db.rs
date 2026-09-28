//! What the checker decided about a document, kept for the editor's queries.

use rustc_hash::FxHashMap;

pub struct SemanticDB {
    pub expr_table: FxHashMap<varn_core::ast::AstId, varn_checker::TypeEntry>,

    pub expr_types: FxHashMap<u32, varn_checker::ExprInfo>,
    pub node_scopes: FxHashMap<u32, varn_checker::ScopeId>,
    pub scope_spans: Vec<varn_checker::checker::ScopeSpan>,
    /// THE type map (Ley 6): one entry per symbol the pipeline resolved, by
    /// the recorded-else-declared rule. Readers fall back to `bind.arena`'s
    /// `sym.ty` (which already carries the checker's finalize pass) and then
    /// to dynamic — the chain lives once per reader, the table once here.
    pub symbol_types: FxHashMap<varn_checker::SymbolId, varn_checker::Type>,

    /// Sin copia propia de arenas: el dueño canónico es `bind` (una sola
    /// `SymbolArena` + una sola `ScopeArena` por documento). El clon que hubo
    /// aquí duplicaba ambas por archivo abierto.
    pub global_scope: varn_checker::ScopeId,

    pub flattened_members: FxHashMap<String, Vec<varn_checker::types::ClassMemberInfo>>,

    pub member_resolutions: FxHashMap<u32, varn_checker::MemberResolution>,

    pub call_resolutions: FxHashMap<u32, varn_checker::CallResolution>,

    /// The missing arms of each non-exhaustive `match`, by its id.
    pub match_gaps: FxHashMap<varn_core::ast::AstId, varn_checker::MatchGap>,

    /// What lowering needs beyond the types: named-argument layouts and the
    /// checker's desugarings. Kept so the compiler views lower this document
    /// exactly as a compile does.
    pub call_mappings: FxHashMap<varn_core::ast::AstId, Vec<Option<usize>>>,
    pub desugar: varn_checker::checker::Desugarings,

    pub bind: varn_checker::BindResult,

    /// The type table every type of this document indexes into: the
    /// checker's, seeded from `bind`, grown by the editor's own queries
    /// (member lookups intern the types they build), and read by every
    /// display. One table, so a type a query minted still prints.
    /// `Arc`, not an owned table: the checker hands every document the SAME,
    /// ever-growing, whole-compilation table (see `DiskResolver::ty_table`).
    /// Cloning it into every document at analysis time — deep, not the Arc's
    /// refcount bump — meant document N's clone copied every type the checker
    /// had interned from documents 1..N-1 too, not just its own: O(n) work
    /// repeated per file, O(n²) over a workspace scan. `Arc::make_mut` at the
    /// few call sites that actually synthesize a document-local type (hover
    /// asking for `Array<int>`, say) copies-on-write only those, and only the
    /// first time — the common case (a file nobody is hovering) pays nothing.
    pub types: std::cell::RefCell<std::sync::Arc<varn_checker::types::CheckerTyTable>>,
}

impl SemanticDB {
    /// The text of `atom`.
    pub fn name(&self, atom: varn_core::Atom) -> &str {
        self.bind.interner.resolve(atom)
    }

    pub fn resolve_at(
        &self,
        name: &str,
        cursor_offset: u32,
    ) -> Option<(varn_checker::SymbolId, varn_checker::Type)> {
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

    pub fn scope_at_offset(&self, cursor_offset: u32) -> varn_checker::ScopeId {
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
