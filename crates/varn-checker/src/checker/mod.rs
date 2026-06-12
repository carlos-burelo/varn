pub(crate) mod compat;
mod decls;
mod stmts;
use crate::binder::{BindResult, Binder};
use crate::scope::ScopeId;
use crate::symbol::SymbolId;
use crate::types::Type;
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use varn_core::ast::Expr;
use varn_core::ast::Program;
use varn_core::Diagnostic;

pub(crate) use crate::checker_annotations::collect_type_annotations;
pub(crate) use crate::checker_enrichment::enrich_call_returns;

#[derive(Clone, Debug)]
pub struct ExprInfo {
    pub ty: Type,
    pub symbol_id: Option<SymbolId>,
}

impl std::fmt::Display for ExprInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.ty)
    }
}

pub struct CheckResult {
    pub bind: BindResult,
    pub diagnostics: varn_core::DiagnosticBag,
    pub expr_types: FxHashMap<u32, ExprInfo>,
    pub flattened_members: FxHashMap<Rc<str>, Vec<crate::types::ClassMemberInfo>>,
    pub type_annotations: varn_core::TypeAnnotations,
    pub profile: CheckProfile,
    pub extension_calls: FxHashMap<u32, Rc<str>>,
    pub extension_members: FxHashMap<u32, Rc<str>>,
    pub extension_set_members: FxHashMap<u32, Rc<str>>,
    pub node_scopes: FxHashMap<u32, crate::scope::ScopeId>,
    pub symbol_types: FxHashMap<SymbolId, crate::types::Type>,
}

impl CheckResult {
    pub fn scope_at_offset(&self, cursor_offset: u32) -> crate::scope::ScopeId {
        let mut best_scope = self.bind.global_scope;
        let mut best_offset: u32 = 0;
        for (&offset, &scope) in &self.node_scopes {
            if offset <= cursor_offset && offset >= best_offset {
                best_offset = offset;
                best_scope = scope;
            }
        }
        best_scope
    }

    pub fn resolve_at(
        &self,
        name: &str,
        cursor_offset: u32,
    ) -> Option<(SymbolId, crate::types::Type)> {
        let scope_id = self.scope_at_offset(cursor_offset);
        let scope = self.bind.scopes.get(scope_id);
        let sym_id = scope.resolve(name, &self.bind.scopes)?;
        let ty = self
            .symbol_types
            .get(&sym_id)
            .cloned()
            .or_else(|| self.bind.arena.get(sym_id).ty.clone())
            .unwrap_or(crate::types::Type::Dynamic);
        Some((sym_id, ty))
    }
}

#[derive(Clone, Debug, Default)]
pub struct CheckProfile {
    pub load_globals: Duration,
    pub bind: Duration,
    pub merge_core_members: Duration,
    pub enrich_call_returns: Duration,
    pub check_stmts: Duration,
    pub collect_annotations: Duration,
    pub finalize: Duration,
}

pub struct Checker {
    pub(crate) diagnostics: varn_core::DiagnosticBag,
    pub(crate) source_file: std::rc::Rc<str>,
    pub(crate) current_scope: crate::scope::ScopeId,
    pub(crate) expected_return_type: Option<Type>,
    pub(crate) narrowed_types: FxHashMap<SymbolId, Vec<Type>>,
    pub(crate) narrowings_cache:
        FxHashMap<(u32, bool, crate::scope::ScopeId), Vec<(SymbolId, Type)>>,
    pub(crate) child_indices: FxHashMap<ScopeId, usize>,
    pub(crate) expr_types: FxHashMap<u32, ExprInfo>,
    pub(crate) infer_cache: FxHashMap<(u32, ScopeId, u32), Type>,
    pub(crate) infer_env_rev: u32,
    pub(crate) compat_cache: FxHashMap<(Type, Type, usize), bool>,
    pub(crate) type_node_cache: FxHashMap<(u32, usize), Type>,
    pub(crate) symbol_type_params_cache: FxHashMap<(Rc<str>, u8), Vec<Rc<str>>>,
    pub(crate) symbol_types: FxHashMap<SymbolId, Type>,
    pub(crate) current_class: Option<Rc<str>>,
    pub(crate) active_type_params: FxHashSet<Rc<str>>,
    pub(crate) abstract_classes: FxHashSet<Rc<str>>,
    pub(crate) is_assignment_target: bool,
    pub(crate) in_pipeline_rhs: bool,
    pub(crate) pipeline_value_type: Option<Type>,
    pub(crate) extension_calls: FxHashMap<u32, Rc<str>>,
    pub(crate) extension_members: FxHashMap<u32, Rc<str>>,
    pub(crate) extension_set_members: FxHashMap<u32, Rc<str>>,
    pub(crate) member_exists_cache: FxHashMap<(Type, Rc<str>), bool>,
    pub(crate) member_type_cache: FxHashMap<(Type, Rc<str>), Option<(Type, Option<usize>)>>,
    pub(crate) expected_type: Option<Type>,
    pub(crate) call_mappings: FxHashMap<u32, Vec<Option<usize>>>,
    pub(crate) reassigned_names: rustc_hash::FxHashSet<Rc<str>>,
    pub(crate) record_expr_types: bool,
    pub(crate) node_scopes: FxHashMap<u32, ScopeId>,
    pub(crate) map_generics_cache: FxHashMap<(Type, Vec<Type>), Type>,
    pub(crate) yielded_types: Option<Vec<Type>>,
    pub warn_implicit_dynamic: bool,
}

impl Checker {
    pub fn check(program: &Program) -> CheckResult {
        Self::check_internal(program, false, false)
    }

    pub fn check_strict(program: &Program) -> CheckResult {
        Self::check_internal(program, false, true)
    }

    pub fn check_for_lsp(program: &Program) -> CheckResult {
        Self::check_internal(program, true, false)
    }

    pub fn check_with_profile(program: &Program) -> CheckResult {
        Self::check_internal(program, true, false)
    }

    fn check_internal(
        program: &Program,
        record_expr_types: bool,
        warn_implicit_dynamic: bool,
    ) -> CheckResult {
        let mut profile = CheckProfile::default();

        let is_builtin = crate::core::is_core_file(&program.filename);
        let globals_ref = if !is_builtin {
            let started = Instant::now();
            let globals = crate::core::global_exports_ref();
            profile.load_globals = started.elapsed();
            Some(globals)
        } else {
            None
        };

        let started = Instant::now();
        let mut bind = match globals_ref {
            Some(globals) => Binder::bind_with_global_refs(program, &globals),
            None => Binder::bind(program),
        };
        profile.bind = started.elapsed();

        let started = Instant::now();
        crate::core::merge_core_members(&mut bind);
        profile.merge_core_members = started.elapsed();

        let started = Instant::now();
        enrich_call_returns(&mut bind);
        profile.enrich_call_returns = started.elapsed();

        let source_file: std::rc::Rc<str> = std::rc::Rc::from(bind.source_file.as_ref());

        let mut checker = Checker {
            current_scope: bind.global_scope,
            diagnostics: varn_core::DiagnosticBag::new(),
            source_file: source_file.clone(),
            expected_return_type: None,
            narrowed_types: FxHashMap::default(),
            narrowings_cache: FxHashMap::default(),
            child_indices: FxHashMap::with_capacity_and_hasher(64, Default::default()),
            expr_types: if record_expr_types {
                FxHashMap::with_capacity_and_hasher(2048, Default::default())
            } else {
                FxHashMap::default()
            },
            infer_cache: FxHashMap::with_capacity_and_hasher(4096, Default::default()),
            infer_env_rev: 0,
            compat_cache: FxHashMap::with_capacity_and_hasher(4096, Default::default()),
            type_node_cache: FxHashMap::with_capacity_and_hasher(1024, Default::default()),
            symbol_type_params_cache: FxHashMap::with_capacity_and_hasher(256, Default::default()),
            symbol_types: FxHashMap::default(),
            current_class: None,
            active_type_params: FxHashSet::default(),
            abstract_classes: FxHashSet::default(),
            is_assignment_target: false,
            in_pipeline_rhs: false,
            pipeline_value_type: None,
            extension_calls: FxHashMap::default(),
            extension_members: FxHashMap::default(),
            extension_set_members: FxHashMap::default(),
            member_exists_cache: FxHashMap::with_capacity_and_hasher(256, Default::default()),
            member_type_cache: FxHashMap::with_capacity_and_hasher(1024, Default::default()),
            warn_implicit_dynamic,
            expected_type: None,
            call_mappings: FxHashMap::default(),
            reassigned_names: rustc_hash::FxHashSet::default(),
            record_expr_types,
            node_scopes: FxHashMap::default(),
            map_generics_cache: FxHashMap::with_capacity_and_hasher(512, Default::default()),
            yielded_types: None,
        };

        let started = Instant::now();
        checker.check_stmts(&program.body, &bind);
        profile.check_stmts = started.elapsed();

        let mut final_diagnostics = std::mem::take(&mut bind.diagnostics);
        final_diagnostics.extend(checker.diagnostics);

        let started = Instant::now();
        let mut annotations = collect_type_annotations(program, &bind);

        for (k, v) in checker.call_mappings {
            annotations.record_call_mapping(k, v);
        }
        for name in &checker.reassigned_names {
            annotations.record_reassigned_name(name);
        }
        profile.collect_annotations = started.elapsed();
        let flattened = std::mem::take(&mut bind.type_members.flattened);

        let started = Instant::now();
        for (sid, ty) in &checker.symbol_types {
            let sym = bind.arena.get_mut(*sid);

            if sym.origin_module.is_some() {
                continue;
            }

            let current_is_weak = match &sym.ty {
                None => true,
                Some(t) => {
                    t.is_dynamic()
                        || match &t.0 {
                            varn_core::TypeKind::Fn(ft) => ft.return_type.is_dynamic(),
                            _ => false,
                        }
                }
            };

            if !sym.has_explicit_type && current_is_weak {
                sym.ty = Some(ty.clone());
            }
        }
        profile.finalize = started.elapsed();

        let symbol_types = checker.symbol_types.clone();
        let node_scopes = if record_expr_types {
            checker.node_scopes.clone()
        } else {
            FxHashMap::default()
        };

        CheckResult {
            bind,
            diagnostics: final_diagnostics,
            expr_types: checker.expr_types,
            flattened_members: flattened,
            type_annotations: annotations,
            profile,
            extension_calls: checker.extension_calls,
            extension_members: checker.extension_members,
            extension_set_members: checker.extension_set_members,
            node_scopes,
            symbol_types,
        }
    }

    #[inline]
    pub(crate) fn emit(&mut self, diag: Diagnostic) {
        let diag = if diag.file.is_empty() {
            diag.with_file(self.source_file.clone())
        } else {
            diag
        };
        self.diagnostics.push(diag);
    }

    pub(crate) fn record_scope(&mut self, offset: u32) {
        if self.record_expr_types {
            self.node_scopes.insert(offset, self.current_scope);
        }
    }

    pub(crate) fn with_expected<R>(
        &mut self,
        ty: Option<Type>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = self.expected_type.take();
        self.expected_type = ty;
        let result = f(self);
        self.expected_type = prev;
        result
    }

    pub(crate) fn infer_type(&mut self, expr: &Expr, bind: &BindResult) -> Type {
        let key = (expr.id(), self.current_scope, self.infer_env_rev);
        if let Some(ty) = self.infer_cache.get(&key) {
            return ty.clone();
        }

        let saved_scope = self.current_scope;
        if let Some(scope) = self.node_scopes.get(&expr.id()) {
            self.current_scope = *scope;
        }

        let ty = self.infer_type_internal(expr, bind);
        self.current_scope = saved_scope;

        self.infer_cache.insert(key, ty.clone());
        ty
    }

    pub(crate) fn next_child_scope(&mut self, bind: &BindResult) -> Option<ScopeId> {
        let children = &bind.scopes.get(self.current_scope).children;
        let idx = self.child_indices.entry(self.current_scope).or_insert(0);
        if *idx < children.len() {
            let child_id = children[*idx];
            *idx += 1;
            Some(child_id)
        } else {
            None
        }
    }

    pub(crate) fn types_compatible_cached(
        &mut self,
        declared: &Type,
        inferred: &Type,
        bind: Option<&BindResult>,
    ) -> bool {
        compat::types_compatible_with_cache(declared, inferred, bind, &mut self.compat_cache)
    }

    pub(crate) fn mark_infer_env_dirty(&mut self) {
        self.infer_env_rev = self.infer_env_rev.wrapping_add(1);
        if self.infer_cache.len() > 16_384 {
            self.infer_cache.clear();
        }
    }

    pub(crate) fn resolve_type_node_cached(
        &mut self,
        node: &varn_core::ast::TypeNode,
        bind: &BindResult,
    ) -> Type {
        let key = (node.range.start.offset, bind as *const BindResult as usize);
        if let Some(cached) = self.type_node_cache.get(&key) {
            return cached.clone();
        }
        let resolved = crate::binder::resolve_type_node(node, Some(bind));
        self.type_node_cache.insert(key, resolved.clone());
        resolved
    }

    pub(crate) fn symbol_type_params(
        &mut self,
        name: &str,
        kind: crate::symbol::SymbolKind,
        bind: &BindResult,
    ) -> Vec<Rc<str>> {
        let key = (Rc::from(name), symbol_kind_cache_key(kind));
        if let Some(cached) = self.symbol_type_params_cache.get(&key) {
            return cached.clone();
        }

        let resolved = if let Some(sid) = bind
            .scopes
            .get(bind.global_scope)
            .resolve(name, &bind.scopes)
        {
            let sym = bind.arena.get(sid);
            if sym.kind == kind {
                sym.type_params.clone()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        self.symbol_type_params_cache.insert(key, resolved.clone());
        resolved
    }

    pub(crate) fn symbol_type_params_any(&mut self, name: &str, bind: &BindResult) -> Vec<Rc<str>> {
        let key = (Rc::from(name), 255);
        if let Some(cached) = self.symbol_type_params_cache.get(&key) {
            return cached.clone();
        }

        let resolved = if let Some(sid) = bind
            .scopes
            .get(bind.global_scope)
            .resolve(name, &bind.scopes)
        {
            bind.arena.get(sid).type_params.clone()
        } else {
            bind.core
                .as_ref()
                .and_then(|b| b.class_type_params.get(name))
                .cloned()
                .unwrap_or_default()
        };

        self.symbol_type_params_cache.insert(key, resolved.clone());
        resolved
    }
}

fn symbol_kind_cache_key(kind: crate::symbol::SymbolKind) -> u8 {
    match kind {
        crate::symbol::SymbolKind::Var => 0,
        crate::symbol::SymbolKind::Let => 1,
        crate::symbol::SymbolKind::Const => 2,
        crate::symbol::SymbolKind::Function => 3,
        crate::symbol::SymbolKind::Class => 4,
        crate::symbol::SymbolKind::Interface => 5,
        crate::symbol::SymbolKind::TypeAlias => 6,
        crate::symbol::SymbolKind::Enum => 7,
        crate::symbol::SymbolKind::Parameter => 8,
        crate::symbol::SymbolKind::Property => 9,
        crate::symbol::SymbolKind::Method => 10,
        crate::symbol::SymbolKind::TypeParameter => 11,
        crate::symbol::SymbolKind::Namespace => 12,
        crate::symbol::SymbolKind::Struct => 13,
        crate::symbol::SymbolKind::Extension => 14,
        crate::symbol::SymbolKind::EnumMember => 15,
    }
}
