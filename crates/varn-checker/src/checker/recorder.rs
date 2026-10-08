use rustc_hash::FxHashMap;
use varn_sem::output::{Desugarings, ExprInfo, ScopeSpan, TypeEntry};
use varn_sem::scope::ScopeId;
use varn_sem::semantic_info::{CallResolution, MemberResolution};
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

pub(crate) struct Recorder {
    pub(crate) enabled: bool,
    pub(crate) expr_types: FxHashMap<u32, ExprInfo>,
    pub(crate) symbol_types: FxHashMap<SymbolId, Type>,
    pub(crate) expr_table: FxHashMap<varn_core::ast::AstId, TypeEntry>,
    pub(crate) expr_seq: u32,
    pub(crate) desugar: Desugarings,
    pub(crate) call_mappings: FxHashMap<varn_core::ast::AstId, Vec<Option<usize>>>,
    pub(crate) node_scopes: FxHashMap<u32, ScopeId>,
    pub(crate) scope_spans: Vec<ScopeSpan>,
    pub(crate) member_resolutions: FxHashMap<u32, MemberResolution>,
    pub(crate) call_resolutions: FxHashMap<u32, CallResolution>,
    pub(crate) match_gaps: FxHashMap<varn_core::ast::AstId, varn_sem::semantic_info::MatchGap>,
}

impl Recorder {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            expr_types: FxHashMap::default(),
            symbol_types: FxHashMap::default(),
            expr_table: FxHashMap::default(),
            expr_seq: 0,
            desugar: Desugarings::default(),
            call_mappings: FxHashMap::default(),
            node_scopes: FxHashMap::default(),
            scope_spans: Vec::new(),
            member_resolutions: FxHashMap::default(),
            call_resolutions: FxHashMap::default(),
            match_gaps: FxHashMap::default(),
        }
    }

    pub(crate) fn record_scope(&mut self, offset: u32, scope: ScopeId) {
        if self.enabled {
            self.node_scopes.insert(offset, scope);
        }
    }

    pub(crate) fn record_scope_span(&mut self, start: u32, end: u32, scope: ScopeId) {
        if self.enabled {
            self.scope_spans.push(ScopeSpan { start, end, scope });
            self.node_scopes.insert(start, scope);
        }
    }

    pub(crate) fn record_type(&mut self, offset: u32, ty: Type) {
        if self.enabled {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: None,
                },
            );
        }
    }

    pub(crate) fn record_type_with_symbol(&mut self, offset: u32, ty: Type, symbol_id: SymbolId) {
        self.symbol_types.insert(symbol_id, ty);
        if self.enabled {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: Some(symbol_id),
                },
            );
        }
    }

    pub(crate) fn record_member_type(&mut self, offset: u32, ty: Type, symbol_id: SymbolId) {
        if self.enabled {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: Some(symbol_id),
                },
            );
        }
    }

    pub(crate) fn project_expr_types(&mut self, bind: &varn_sem::bind::BindResult) {
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

    pub(crate) fn next_seq(&mut self) -> u32 {
        let seq = self.expr_seq;
        self.expr_seq += 1;
        seq
    }
}
