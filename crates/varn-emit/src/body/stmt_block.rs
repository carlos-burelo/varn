use super::context::FnEmitter;
use super::match_pattern::MatchDest;
use super::small_utils::{bool_lit, placeholder};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::ast::{ExprKind, Pattern, StmtId, StmtKind};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub fn lower_block(&mut self, stmts: &[StmtId]) -> Vec<TirStmt> {
        self.scopes.push(FxHashMap::default());
        self.push_dispose_frame();
        let mut out = Vec::new();
        for &s in stmts {
            out.extend(self.lower_stmt(s));
        }

        out.extend(self.drain_dispose_frame());
        self.scopes.pop();
        out
    }

    pub fn push_dispose_frame(&mut self) {
        self.disposables.push(Vec::new());
    }

    pub fn drain_dispose_frame(&mut self) -> Vec<TirStmt> {
        let mut out = Vec::new();
        for resource in self.disposables.pop().unwrap_or_default().into_iter().rev() {
            out.push(TirStmt::Expr(TirExpr {
                kind: TirExprKind::MethodCall {
                    recv: Box::new(resource),
                    name: Arc::from("dispose"),
                    args: vec![],
                },
                ty: BackendTy::Void,
                res: Resolution::ByName {
                    name: Arc::from("dispose"),
                    why: DynReason::NotYetSupported,
                },
                span: Span::EMPTY,
            }));
        }
        out
    }

    pub fn lower_stmt_as_block(&mut self, s: StmtId) -> Vec<TirStmt> {
        match &self.ast_arena.stmt(s).kind {
            StmtKind::Block { stmts } => {
                let stmts = stmts.clone();
                self.lower_block(&stmts)
            }
            StmtKind::Empty
            | StmtKind::Expr { .. }
            | StmtKind::Decl(_)
            | StmtKind::Error
            | StmtKind::If { .. }
            | StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::ForIn { .. }
            | StmtKind::ForOf { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Return { .. }
            | StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Throw { .. }
            | StmtKind::Try { .. }
            | StmtKind::Using { .. }
            | StmtKind::Labeled { .. }
            | StmtKind::Debugger => self.lower_stmt(s),
        }
    }

    pub(super) fn lower_plain_stmt(&mut self, s: StmtId) -> Vec<TirStmt> {
        let one = |s: TirStmt| vec![s];
        let drained = |em: &mut Self, built: Vec<TirStmt>| {
            let mut out = std::mem::take(&mut em.pending);
            out.extend(built);
            out
        };
        match &self.ast_arena.stmt(s).kind {
            StmtKind::Block { stmts } => self.lower_block(stmts),
            StmtKind::Expr { expression } => {
                let expression = *expression;
                if let ExprKind::Match { subject, cases } = &self.ast_arena.expr(expression).kind {
                    let (subject, cases) = (*subject, cases);
                    return self.lower_match_stmt(subject, cases);
                }
                let e = self.lower_expr(expression);
                drained(self, one(TirStmt::Expr(e)))
            }
            StmtKind::Empty | StmtKind::Debugger | StmtKind::Error => vec![],

            StmtKind::Decl(decl) => {
                let nested = super::super::decl_classify::class_decl(decl)
                    .and_then(|c| Some((c.id?, c.ast_id)))
                    .or_else(|| {
                        super::super::decl_classify::enum_decl(decl).map(|e| (e.id, e.ast_id))
                    })
                    .and_then(|(id, ast_id)| {
                        let name = crate::namespace_scope::declared_name(
                            self.m.type_names,
                            ast_id,
                            self.m.interner.resolve(id),
                        );
                        self.m.nested_types.declare(&name, s)
                    });
                let built = match nested {
                    Some(stmt) => vec![stmt],
                    None => self.lower_decl_stmt(decl),
                };
                drained(self, built)
            }

            StmtKind::Return { argument } => {
                let argument = *argument;
                if let Some(arg) = argument
                    && let ExprKind::Match { subject, cases } = &self.ast_arena.expr(arg).kind
                {
                    let (subject, cases) = (*subject, cases);
                    return self.lower_match(subject, cases, MatchDest::Return);
                }
                let a = argument.map(|a| {
                    let value = self.lower_expr(a);
                    self.guard_dynamic(value, self.return_ty)
                });
                drained(self, one(TirStmt::Return(a)))
            }
            StmtKind::Throw { argument } => {
                let a = self.lower_expr(*argument);
                drained(self, one(TirStmt::Throw(a)))
            }
            StmtKind::Break { .. } => one(TirStmt::Break),
            StmtKind::Continue { .. } => one(TirStmt::Continue),

            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
                let cond = self.lower_cond(test);
                let mut out = std::mem::take(&mut self.pending);
                let then_body = self.lower_stmt_as_block(consequent);
                let else_body = alternate
                    .map(|a| self.lower_stmt_as_block(a))
                    .unwrap_or_default();
                out.push(TirStmt::If {
                    cond,
                    then_body,
                    else_body,
                });
                out
            }

            StmtKind::While { test, body } => {
                let (test, body) = (*test, *body);
                let cond = self.lower_cond(test);
                let cond_pending = std::mem::take(&mut self.pending);
                let body = self.lower_stmt_as_block(body);
                if cond_pending.is_empty() {
                    one(TirStmt::Loop { cond, body })
                } else {
                    let mut loop_body = cond_pending;
                    loop_body.push(TirStmt::If {
                        cond,
                        then_body: vec![],
                        else_body: vec![TirStmt::Break],
                    });
                    loop_body.extend(body);
                    one(TirStmt::Loop {
                        cond: bool_lit(true),
                        body: loop_body,
                    })
                }
            }

            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                let (test, update, body) = (*test, *update, *body);
                self.lower_for(init.as_deref(), test, update, body)
            }

            StmtKind::DoWhile { body, test } => {
                let (body, test) = (*body, *test);
                let mut loop_body = self.lower_stmt_as_block(body);
                let cond = self.lower_cond(test);
                loop_body.extend(std::mem::take(&mut self.pending));
                loop_body.push(TirStmt::If {
                    cond,
                    then_body: vec![],
                    else_body: vec![TirStmt::Break],
                });
                one(TirStmt::Loop {
                    cond: bool_lit(true),
                    body: loop_body,
                })
            }

            StmtKind::ForOf {
                left,
                right,
                body,
                is_await,
                ..
            } => self.lower_for_of(left, *right, *body, *is_await),
            StmtKind::ForIn {
                left, right, body, ..
            } => self.lower_for_in(left, *right, *body),

            StmtKind::Try {
                block,
                catches,
                finally,
            } => self.lower_try(*block, catches, *finally),

            StmtKind::Switch {
                discriminant,
                cases,
            } => self.lower_switch(*discriminant, cases),

            StmtKind::Using { declarations, .. } => {
                let dyn_ty = BackendTy::Dynamic(DynReason::NotYetSupported);
                let mut out = Vec::new();
                for d in declarations {
                    let init = d.init.map(|e| self.lower_expr(e));
                    match &d.id {
                        Pattern::Identifier { name, .. } => {
                            let ty = init.as_ref().map(|e| e.ty).unwrap_or(dyn_ty);
                            let name_str = self.m.interner.resolve(*name);
                            let global = match self.m.globals.get(name_str) {
                                Some(&slot) if self.top_level => Some(slot),
                                Some(_) | None => None,
                            };
                            let res = match global {
                                Some(slot) => {
                                    out.extend(std::mem::take(&mut self.pending));
                                    let value = init.unwrap_or(TirExpr {
                                        kind: TirExprKind::NullLit,
                                        ty,
                                        res: Resolution::None,
                                        span: Span::EMPTY,
                                    });
                                    out.push(TirStmt::Expr(TirExpr {
                                        kind: TirExprKind::Assign {
                                            target: Box::new(TirExpr {
                                                kind: TirExprKind::Var,
                                                ty,
                                                res: Resolution::GlobalSlot(slot),
                                                span: Span::EMPTY,
                                            }),
                                            value: Box::new(value),
                                        },
                                        ty: BackendTy::Void,
                                        res: Resolution::None,
                                        span: Span::EMPTY,
                                    }));
                                    Resolution::GlobalSlot(slot)
                                }
                                None => {
                                    let local = self.bind_local(Arc::from(name_str), ty);
                                    out.extend(std::mem::take(&mut self.pending));
                                    out.push(TirStmt::Let { local, ty, init });
                                    Resolution::Local(local)
                                }
                            };
                            if let Some(frame) = self.disposables.last_mut() {
                                frame.push(TirExpr {
                                    kind: TirExprKind::Var,
                                    ty,
                                    res,
                                    span: Span::EMPTY,
                                });
                            }
                        }

                        pat @ Pattern::Array { .. }
                        | pat @ Pattern::Object { .. }
                        | pat @ Pattern::Assignment { .. }
                        | pat @ Pattern::Rest { .. } => {
                            let src = init.unwrap_or_else(|| placeholder());
                            out.extend(std::mem::take(&mut self.pending));
                            let src = self.hoist(src);
                            out.extend(std::mem::take(&mut self.pending));
                            if let Some(frame) = self.disposables.last_mut() {
                                frame.push(src.clone());
                            }
                            self.bind_pattern(pat, src, &mut out);
                        }
                    }
                }
                out
            }

            StmtKind::Labeled { body, .. } => self.lower_stmt_as_block(*body),
        }
    }
}
