use super::Binder;
use std::sync::Arc;
use varn_core::ast::{ExprKind, ForInit, Pattern, StmtId, StmtKind, VarDeclarator, VarKind};
use varn_sem::scope::ScopeKind;
use varn_sem::symbol::SymbolKind;

impl<'r> Binder<'r> {
    pub(crate) fn bind_stmts(&mut self, stmts: &[StmtId]) {
        for &stmt in stmts {
            self.bind_stmt(stmt);
        }
    }

    pub(super) fn bind_var_declarators(
        &mut self,
        declarators: &[VarDeclarator],
        kind: VarKind,
        doc: Option<&Arc<str>>,
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
                .map(|ann| self.resolve_type(ann))
                .or_else(|| {
                    declarator
                        .init
                        .map(|expr| self.infer_expr_type_self(expr))
                        .filter(|ty| !ty.is_dynamic())
                });

            self.bind_pattern(
                &declarator.id,
                sym_kind,
                line,
                doc.as_ref().map(|s| s.to_string()),
                ty,
                declarator.type_ann.is_some(),
            );

            if let Pattern::Identifier { name, .. } = &declarator.id {
                if let Some(init_expr) = declarator.init {
                    if let ExprKind::Object { properties, .. } =
                        &self.ast_arena.expr(init_expr).kind
                    {
                        let fields = self.collect_object_members(properties);
                        if !fields.is_empty() {
                            self.type_members.objects.insert(*name, fields);
                        }
                    }
                }
            }

            if let Some(init_expr) = declarator.init {
                self.bind_expr(init_expr);
            }
        }
    }

    pub(crate) fn bind_stmt(&mut self, stmt: StmtId) {
        let arena = self.ast_arena;
        match &arena.stmt(stmt).kind {
            StmtKind::Decl(decl) => self.bind_decl(decl),
            StmtKind::Block { stmts } => {
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                self.bind_stmts(stmts);
                self.finalize_array_watch(child);
                self.current = saved;
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
                self.bind_expr(test);
                self.bind_stmt(consequent);
                if let Some(alt) = alternate {
                    self.bind_stmt(alt);
                }
            }
            StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
                let (test, body) = (*test, *body);
                self.bind_expr(test);
                self.bind_stmt(body);
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                let body = *body;
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                if let Some(init) = init {
                    match init.as_ref() {
                        ForInit::Var { kind, declarators } => {
                            self.bind_var_declarators(declarators, *kind, None);
                        }
                        ForInit::Expr(e) => {
                            self.bind_expr(*e);
                        }
                    }
                }
                if let Some(t) = test {
                    self.bind_expr(*t);
                }
                if let Some(u) = update {
                    self.bind_expr(*u);
                }
                self.bind_stmt(body);
                self.finalize_array_watch(child);
                self.current = saved;
            }
            StmtKind::ForIn {
                left, right, body, ..
            }
            | StmtKind::ForOf {
                left, right, body, ..
            } => {
                let (right, body) = (*right, *body);
                let child = self.scopes.child(ScopeKind::Block, self.current);
                let saved = self.current;
                self.current = child;
                let line = arena.expr(right).range.start.line;
                self.bind_pattern(left, SymbolKind::Let, line, None, None, false);
                self.bind_expr(right);
                self.bind_stmt(body);
                self.finalize_array_watch(child);
                self.current = saved;
            }
            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                let discriminant = *discriminant;
                self.bind_expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.bind_expr(*t);
                    }
                    self.bind_stmts(&case.body);
                }
            }
            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                let (block, finally) = (*block, *finally);
                self.bind_stmt(block);
                for clause in catches {
                    let child = self.scopes.child(ScopeKind::Block, self.current);
                    let saved = self.current;
                    self.current = child;
                    if let Some(p) = &clause.param {
                        let ty = clause.type_ann.as_ref().map(|ann| self.resolve_type(ann));
                        let block_line = arena.stmt(block).range.start.line;
                        let explicit = ty.is_some();
                        self.bind_pattern(p, SymbolKind::Let, block_line, None, ty, explicit);
                    }
                    self.bind_stmt(clause.body);
                    self.finalize_array_watch(child);
                    self.current = saved;
                }
                if let Some(fin) = finally {
                    self.bind_stmt(fin);
                }
            }
            StmtKind::Labeled { body, .. } => {
                self.bind_stmt(*body);
            }
            StmtKind::Expr { expression } => {
                self.bind_expr(*expression);
            }
            StmtKind::Return { argument } => {
                if let Some(arg) = argument {
                    self.bind_expr(*arg);
                }
            }
            StmtKind::Throw { argument } => {
                self.bind_expr(*argument);
            }
            StmtKind::Using { declarations, .. } => {
                self.bind_var_declarators(declarations, VarKind::Const, None);
            }
            StmtKind::Empty
            | StmtKind::Error
            | StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Debugger => {}
        }
    }
}
