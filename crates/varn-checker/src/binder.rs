use crate::scope::{CheckerScope, ScopeArena, ScopeId, ScopeKind};
use crate::symbol::{Symbol, SymbolArena, SymbolKind};
use crate::types::Type;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{ExprKind, ForInit, Program, Stmt, StmtKind, VarDeclarator};

mod class;
mod decl_values;
mod decls;
mod imports;
mod inference_utils;
mod interface;
mod type_inference;
mod type_resolution;
mod types;

use crate::module_resolver::resolve_module_bind_ref;
pub use crate::types::{ClassMemberInfo, ClassMemberKind, TypeContext};
pub use inference_utils::build_fn_type;
pub use type_inference::{infer_expr_type, pattern_lead_name, widen_literal};
pub use type_resolution::{resolve_primitive, resolve_type_node};
pub use types::{BindResult, Extensions, PendingEnrich, TypeMembers};
use varn_core::ast::{Pattern, TypeNode, VarKind};

pub struct Binder {
    pub(crate) arena: SymbolArena,
    pub(crate) scopes: ScopeArena,
    pub(crate) current: ScopeId,
    pub(crate) class_methods: FxHashMap<Rc<str>, FxHashMap<Rc<str>, Type>>,
    pub(crate) type_members: TypeMembers,
    pub(crate) class_parents: FxHashMap<Rc<str>, Rc<str>>,
    pub(crate) diagnostics: varn_core::DiagnosticBag,
    pub(crate) source_file: Rc<str>,
    pub(crate) sum_type_variants: FxHashMap<Rc<str>, Vec<Rc<str>>>,
    pub(crate) sum_variant_parent: FxHashMap<Rc<str>, Rc<str>>,
    pub(crate) sum_variant_fields: FxHashMap<Rc<str>, Vec<(Rc<str>, Type)>>,
    pub(crate) extensions: Extensions,
    pub(crate) pending_enrich: Vec<PendingEnrich>,
}

impl TypeContext for Binder {
    fn get_interface_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = resolve_module_bind_ref(origin) {
                    return rb.type_members.interfaces.get(name).cloned();
                }
            }
        }
        self.type_members.interfaces.get(name).cloned()
    }

    fn get_class_members(&self, name: &str, origin: Option<&str>) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = resolve_module_bind_ref(origin) {
                    return rb.type_members.classes.get(name).map(|e| e.members.clone());
                }
            }
        }
        self.type_members
            .classes
            .get(name)
            .map(|e| e.members.clone())
    }

    fn get_namespace_members(
        &self,
        name: &str,
        origin: Option<&str>,
    ) -> Option<Vec<ClassMemberInfo>> {
        if let Some(origin) = origin {
            if origin != self.source_file.as_ref() {
                if let Some(rb) = resolve_module_bind_ref(origin) {
                    return rb.type_members.namespaces.get(name).cloned();
                }
            }
        }
        self.type_members.namespaces.get(name).cloned()
    }

    fn resolve_symbol(&self, name: &str) -> Option<Type> {
        let scope = self.scopes.get(self.current);
        let id = scope.resolve(name, &self.scopes)?;
        self.arena.get(id).ty.clone()
    }

    fn source_file(&self) -> Option<&str> {
        Some(self.source_file.as_ref())
    }

    fn get_alias_node(&self, name: &str) -> Option<(Vec<String>, TypeNode)> {
        let scope = self.scopes.get(self.current);
        let id = scope.resolve(name, &self.scopes)?;
        let sym = self.arena.get(id);
        let node = sym.alias_node.as_ref()?;
        Some((
            sym.type_params.iter().map(|s| s.to_string()).collect(),
            *node.clone(),
        ))
    }
}

impl Binder {
    pub fn bind(program: &Program) -> BindResult {
        Self::bind_with_globals_iter(program, FxHashMap::default())
    }

    pub fn bind_with_globals(program: &Program, globals: FxHashMap<Rc<str>, Symbol>) -> BindResult {
        Self::bind_with_globals_iter(program, globals)
    }

    pub fn bind_with_global_refs(
        program: &Program,
        globals: &FxHashMap<Rc<str>, Symbol>,
    ) -> BindResult {
        Self::bind_with_globals_iter(
            program,
            globals
                .iter()
                .map(|(name, sym)| (name.clone(), sym.clone())),
        )
    }

    fn bind_with_globals_iter<I>(program: &Program, globals: I) -> BindResult
    where
        I: IntoIterator<Item = (Rc<str>, Symbol)>,
    {
        let mut b = Binder {
            arena: SymbolArena::default(),
            scopes: ScopeArena::default(),
            current: 0,
            class_methods: FxHashMap::default(),
            type_members: TypeMembers::default(),
            class_parents: FxHashMap::default(),
            diagnostics: varn_core::DiagnosticBag::new(),
            source_file: Rc::from(program.filename.as_ref()),
            sum_type_variants: FxHashMap::default(),
            sum_variant_parent: FxHashMap::default(),
            sum_variant_fields: FxHashMap::default(),
            extensions: Extensions::default(),
            pending_enrich: Vec::new(),
        };

        let global = b.scopes.push(CheckerScope::new(ScopeKind::Global, None));
        b.current = global;

        for (name, sym) in globals {
            let id = b.arena.push(sym);
            b.scopes.get_mut(global).define(name, id);
        }

        b.bind_stmts(&program.body);

        BindResult {
            arena: b.arena,
            scopes: b.scopes,
            global_scope: global,
            diagnostics: b.diagnostics,
            class_methods: b.class_methods,
            type_members: b.type_members,
            class_parents: b.class_parents,
            source_file: b.source_file,
            sum_type_variants: b.sum_type_variants,
            sum_variant_parent: b.sum_variant_parent,
            sum_variant_fields: b.sum_variant_fields,
            extensions: b.extensions,
            builtin: None,
            pending_enrich: b.pending_enrich,
        }
    }

    pub(crate) fn bind_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.bind_stmt(stmt);
        }
    }

    fn bind_var_declarators(
        &mut self,
        declarators: &[VarDeclarator],
        kind: VarKind,
        doc: Option<&Rc<str>>,
    ) {
        let sym_kind = match kind {
            VarKind::Const => SymbolKind::Const,
            VarKind::Let => SymbolKind::Let,
        };

        for declarator in declarators {
            let line = declarator.range.start.line;
            let ty = declarator
                .type_ann
                .as_ref()
                .or(match &declarator.id {
                    Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                    _ => None,
                })
                .map(|ann| resolve_type_node(ann, Some(self)))
                .or_else(|| {
                    declarator
                        .init
                        .as_ref()
                        .map(|expr| infer_expr_type(expr, Some(self)))
                        .filter(|ty| !ty.is_dynamic())
                });

            self.bind_pattern(
                &declarator.id,
                sym_kind,
                line,
                doc.as_ref().map(|s| s.to_string()),
                ty,
            );

            if let Pattern::Identifier { name, .. } = &declarator.id {
                if let Some(init_expr) = &declarator.init {
                    if let ExprKind::Object { properties, .. } = &init_expr.kind {
                        let fields = self.collect_object_members(properties);
                        if !fields.is_empty() {
                            self.type_members.objects.insert(name.clone(), fields);
                        }
                    }
                }
            }

            if let Some(init_expr) = &declarator.init {
                self.bind_expr(init_expr);
            }
        }
    }

    pub(crate) fn bind_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Decl(decl) => self.bind_decl(decl),
            StmtKind::Block { stmts } => {
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                self.bind_stmts(stmts);
                self.current = saved;
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.bind_expr(test);
                self.bind_stmt(consequent);
                if let Some(alt) = alternate {
                    self.bind_stmt(alt);
                }
            }
            StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
                self.bind_expr(test);
                self.bind_stmt(body);
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                if let Some(init) = init {
                    match init.as_ref() {
                        ForInit::Var { kind, declarators } => {
                            self.bind_var_declarators(declarators, *kind, None);
                        }
                        ForInit::Expr(e) => {
                            self.bind_expr(e);
                        }
                    }
                }
                if let Some(t) = test {
                    self.bind_expr(t);
                }
                if let Some(u) = update {
                    self.bind_expr(u);
                }
                self.bind_stmt(body);
                self.current = saved;
            }
            StmtKind::ForIn {
                left, right, body, ..
            }
            | StmtKind::ForOf {
                left, right, body, ..
            } => {
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                self.bind_pattern(left, SymbolKind::Let, right.range.start.line, None, None);
                self.bind_expr(right);
                self.bind_stmt(body);
                self.current = saved;
            }
            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                self.bind_expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.bind_expr(t);
                    }
                    self.bind_stmts(&case.body);
                }
            }
            StmtKind::Try {
                block,
                catch,
                finally,
            } => {
                self.bind_stmt(block);
                if let Some(clause) = catch {
                    let child = self.scopes.child(ScopeKind::Block, self.current);
                    let saved = self.current;
                    self.current = child;
                    if let Some(p) = &clause.param {
                        self.bind_pattern(p, SymbolKind::Let, block.range.start.line, None, None);
                    }
                    self.bind_stmt(&clause.body);
                    self.current = saved;
                }
                if let Some(fin) = finally {
                    self.bind_stmt(fin);
                }
            }
            StmtKind::Labeled { body, .. } => {
                self.bind_stmt(body);
            }
            StmtKind::Expr { expression } => {
                self.bind_expr(expression);
            }
            StmtKind::Return { argument } => {
                if let Some(arg) = argument {
                    self.bind_expr(arg);
                }
            }
            StmtKind::Throw { argument } => {
                self.bind_expr(argument);
            }
            StmtKind::Using { declarations, .. } => {
                self.bind_var_declarators(declarations, VarKind::Const, None);
            }
            _ => {}
        }
    }
}
