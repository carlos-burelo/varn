use super::context::FnEmitter;
use super::small_utils::{bool_lit, has_continue};
use std::sync::Arc;
use varn_core::ast::{ExprId, Pattern, StmtId};
use varn_tir::{
    BackendTy, DynReason, LocalId, Resolution, Span, TirExpr, TirExprKind, TirStmt, TirUnOp,
};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_for(
        &mut self,
        init: Option<&varn_core::ast::ForInit>,
        test: Option<ExprId>,
        update: Option<ExprId>,
        body: StmtId,
    ) -> Vec<TirStmt> {
        use varn_core::ast::ForInit;
        let mut out = Vec::new();
        let mut declared: Vec<(usize, LocalId, BackendTy)> = Vec::new();

        match init {
            Some(ForInit::Var { declarators, .. }) => {
                for d in declarators {
                    if let Pattern::Identifier { name, .. } = &d.id {
                        let iexpr = d.init.map(|e| self.lower_expr(e));
                        let ty = iexpr
                            .as_ref()
                            .map(|e| e.ty)
                            .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported));
                        let local = self.bind_local(Arc::from(self.m.interner.resolve(*name)), ty);
                        out.extend(std::mem::take(&mut self.pending));
                        declared.push((out.len(), local, ty));
                        out.push(TirStmt::Let {
                            local,
                            ty,
                            init: iexpr,
                        });
                    }
                }
            }
            Some(ForInit::Expr(e)) => {
                let e = self.lower_expr(*e);
                out.extend(std::mem::take(&mut self.pending));
                out.push(TirStmt::Expr(e));
            }
            None => {}
        }

        let continues = has_continue(self.ast_arena, body);
        let cond = test.map(|t| self.lower_cond(t));
        let cond_pending = std::mem::take(&mut self.pending);
        let body_stmts = self.lower_stmt_as_block(body);
        let update_stmts: Vec<TirStmt> = match update {
            Some(u) => {
                let ue = self.lower_expr(u);
                let mut upd = std::mem::take(&mut self.pending);
                upd.push(TirStmt::Expr(ue));
                upd
            }
            None => Vec::new(),
        };
        let shared = declared
            .iter()
            .any(|(_, local, _)| self.captured_locals.contains(&local.0));

        if !continues && !shared {
            let has_cond_pending = !cond_pending.is_empty();
            let cond = cond.unwrap_or_else(|| bool_lit(true));
            let mut loop_body = if has_cond_pending {
                let mut p = cond_pending;
                p.push(TirStmt::If {
                    cond: cond.clone(),
                    then_body: vec![],
                    else_body: vec![TirStmt::Break],
                });
                p
            } else {
                Vec::new()
            };
            loop_body.extend(body_stmts);
            loop_body.extend(update_stmts);
            let loop_cond = if has_cond_pending {
                bool_lit(true)
            } else {
                cond
            };
            out.push(TirStmt::Loop {
                cond: loop_cond,
                body: loop_body,
            });
            return out;
        }

        let var = |local: LocalId, ty: BackendTy| TirExpr {
            kind: TirExprKind::Var,
            ty,
            res: Resolution::Local(local),
            span: Span::EMPTY,
        };
        let assign = |to: TirExpr, from: TirExpr| {
            TirStmt::Expr(TirExpr {
                kind: TirExprKind::Assign {
                    target: Box::new(to),
                    value: Box::new(from),
                },
                ty: BackendTy::Void,
                res: Resolution::None,
                span: Span::EMPTY,
            })
        };

        let mut carried: Vec<(LocalId, LocalId, BackendTy)> = Vec::new();
        if shared {
            for (at, local, ty) in &declared {
                let carrier = self.fresh_local(*ty);
                if let TirStmt::Let { local: slot, .. } = &mut out[*at] {
                    *slot = carrier;
                }
                carried.push((carrier, *local, *ty));
            }
        }
        let hand_back: Vec<TirStmt> = carried
            .iter()
            .map(|(carrier, local, ty)| assign(var(*carrier, *ty), var(*local, *ty)))
            .collect();

        let first = self.fresh_local(BackendTy::Bool);
        out.push(TirStmt::Let {
            local: first,
            ty: BackendTy::Bool,
            init: Some(bool_lit(true)),
        });

        let mut loop_body: Vec<TirStmt> = carried
            .iter()
            .map(|(carrier, local, ty)| TirStmt::Let {
                local: *local,
                ty: *ty,
                init: Some(var(*carrier, *ty)),
            })
            .collect();
        if !update_stmts.is_empty() {
            loop_body.push(TirStmt::If {
                cond: TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Not,
                        operand: Box::new(var(first, BackendTy::Bool)),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: Span::EMPTY,
                },
                then_body: update_stmts,
                else_body: vec![],
            });
        }
        loop_body.push(assign(var(first, BackendTy::Bool), bool_lit(false)));
        if let Some(cond) = cond {
            loop_body.extend(cond_pending);
            loop_body.push(TirStmt::If {
                cond,
                then_body: vec![],
                else_body: vec![TirStmt::Break],
            });
        }
        loop_body.extend(before_continues(body_stmts, &hand_back));
        loop_body.extend(hand_back);
        out.push(TirStmt::Loop {
            cond: bool_lit(true),
            body: loop_body,
        });
        out
    }

    pub(super) fn lower_for_in(
        &mut self,
        left: &Pattern,
        right: ExprId,
        body: StmtId,
    ) -> Vec<TirStmt> {
        let obj = self.lower_expr(right);
        let mut out = std::mem::take(&mut self.pending);
        let s = self.tt.intern(BackendTy::Str);
        let keys_ty = BackendTy::Array(s);
        let keys = TirExpr {
            kind: TirExprKind::ObjectKeys {
                operand: Box::new(obj),
            },
            ty: keys_ty,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        let keys = self.hoist(keys);
        out.extend(std::mem::take(&mut self.pending));
        let Pattern::Identifier { name, .. } = left else {
            return self.lower_for_of_protocol(left, keys, out, body, false);
        };
        let name: Arc<str> = Arc::from(self.m.interner.resolve(*name));
        out.extend(self.for_of_over_array(&name, keys, BackendTy::Str, body));
        out
    }
}

fn before_continues(stmts: Vec<TirStmt>, prefix: &[TirStmt]) -> Vec<TirStmt> {
    if prefix.is_empty() {
        return stmts;
    }
    let mut out = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        match stmt {
            TirStmt::Continue => {
                out.extend(prefix.iter().cloned());
                out.push(TirStmt::Continue);
            }
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(TirStmt::If {
                cond,
                then_body: before_continues(then_body, prefix),
                else_body: before_continues(else_body, prefix),
            }),
            TirStmt::Try {
                body,
                catch_local,
                catch_body,
            } => out.push(TirStmt::Try {
                body: before_continues(body, prefix),
                catch_local,
                catch_body: before_continues(catch_body, prefix),
            }),
            other @ TirStmt::Expr(_)
            | other @ TirStmt::Let { .. }
            | other @ TirStmt::Return(_)
            | other @ TirStmt::Loop { .. }
            | other @ TirStmt::Break
            | other @ TirStmt::Throw(_)
            | other @ TirStmt::BuildClass(_) => out.push(other),
        }
    }
    out
}
