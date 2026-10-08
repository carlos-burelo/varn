use crate::bind::BindResult;
use crate::scope::ScopeId;
use crate::semantic_info::{CallResolution, MemberResolution};
use crate::symbol::SymbolId;
use crate::types::{ClassMemberInfo, Type};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ExprInfo {
    pub ty: Type,
    pub symbol_id: Option<SymbolId>,
}

#[derive(Clone, Debug)]
pub struct TypeEntry {
    pub ty: Type,
    pub refined: Option<Type>,
    pub start: u32,
    pub end: u32,
    pub seq: u32,
    pub symbol_id: Option<SymbolId>,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScopeSpan {
    pub start: u32,
    pub end: u32,
    pub scope: ScopeId,
}

#[derive(Clone, Debug)]
pub struct ForeignEnum {
    pub name: Arc<str>,
    pub origin: Arc<str>,
    pub variants: Vec<(Arc<str>, usize)>,
}

#[derive(Clone, Debug)]
pub struct InheritedField {
    pub name: Arc<str>,
    pub ty: Type,
    pub optional: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Desugarings {
    pub extension_calls: FxHashMap<u32, Arc<str>>,
    pub extension_members: FxHashMap<u32, Arc<str>>,
    pub extension_set_members: FxHashMap<u32, Arc<str>>,
    pub operator_calls: FxHashSet<varn_core::ast::AstId>,
    pub foreign_enums: Vec<ForeignEnum>,
    pub foreign_inherited_fields: std::collections::BTreeMap<Arc<str>, Vec<InheritedField>>,
    pub match_arm_subjects: FxHashMap<varn_core::ast::AstId, Vec<Type>>,
}

#[derive(Clone, Debug)]
pub struct TestTarget {
    pub name: Arc<str>,
    pub file: Arc<str>,
    pub is_async: bool,
}

pub struct CheckResult {
    pub bind: BindResult,
    pub diagnostics: varn_core::DiagnosticBag,
    pub expr_types: FxHashMap<u32, ExprInfo>,
    pub flattened_members: FxHashMap<Arc<str>, Vec<ClassMemberInfo>>,
    pub profile: CheckProfile,
    pub node_scopes: FxHashMap<u32, ScopeId>,
    pub scope_spans: Vec<ScopeSpan>,
    pub symbol_types: FxHashMap<SymbolId, Type>,
    pub member_resolutions: FxHashMap<u32, MemberResolution>,
    pub call_resolutions: FxHashMap<u32, CallResolution>,
    pub match_gaps: FxHashMap<varn_core::ast::AstId, crate::semantic_info::MatchGap>,
    pub expr_table: FxHashMap<varn_core::ast::AstId, TypeEntry>,
    pub call_mappings: FxHashMap<varn_core::ast::AstId, Vec<Option<usize>>>,
    pub desugar: Desugarings,
    pub test_targets: Vec<TestTarget>,
}

impl CheckResult {
    pub fn scope_at_offset(&self, cursor_offset: u32) -> ScopeId {
        let mut best_scope = self.bind.global_scope;
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

    pub fn resolve_at(&self, name: &str, cursor_offset: u32) -> Option<(SymbolId, Type)> {
        let scope_id = self.scope_at_offset(cursor_offset);
        let scope = self.bind.scopes.get(scope_id);
        let atom = self.bind.interner.get(name)?;
        let sym_id = scope.resolve(atom, &self.bind.scopes)?;
        let ty = self
            .symbol_types
            .get(&sym_id)
            .cloned()
            .or_else(|| self.bind.arena.get(sym_id).ty)
            .unwrap_or(Type::Dynamic);
        Some((sym_id, ty))
    }
}

#[derive(Clone, Debug, Default)]
pub struct CheckProfile {
    pub load_globals: Duration,
    pub bind: Duration,
    pub merge_core_members: Duration,
    pub enrich_call_returns: Duration,
    pub init: Duration,
    pub check_stmts: Duration,
    pub collect_annotations: Duration,
    pub finalize: Duration,
    pub cleanup: Duration,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CheckOptions {
    pub record_types: bool,
}

impl CheckOptions {
    pub fn compile() -> Self {
        Self::default()
    }

    pub fn tooling() -> Self {
        Self { record_types: true }
    }
}
