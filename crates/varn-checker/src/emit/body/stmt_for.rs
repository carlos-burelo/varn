use super::context::FnEmitter;
use super::small_utils::{bool_lit, has_continue};
use std::sync::Arc;
use varn_core::ast::{ExprId, Pattern, StmtId};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirExpr, TirExprKind, TirStmt, TirUnOp};

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

        if !has_continue(self.ast_arena, body) {
            let cond = test
                .map(|t| self.lower_cond(t))
                .unwrap_or_else(|| bool_lit(true));
            let cond_pending = std::mem::take(&mut self.pending);
            let has_cond_pending = !cond_pending.is_empty();
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
            loop_body.extend(self.lower_stmt_as_block(body));
            if let Some(u) = update {
                let ue = self.lower_expr(u);
                loop_body.extend(std::mem::take(&mut self.pending));
                loop_body.push(TirStmt::Expr(ue));
            }
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

        let first = self.fresh_local(BackendTy::Bool);
        out.push(TirStmt::Let {
            local: first,
            ty: BackendTy::Bool,
            init: Some(bool_lit(true)),
        });
        let first_var = || TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Bool,
            res: Resolution::Local(first),
            span: Span::EMPTY,
        };

        let mut loop_body: Vec<TirStmt> = Vec::new();

        if let Some(u) = update {
            let ue = self.lower_expr(u);
            let mut upd = std::mem::take(&mut self.pending);
            upd.push(TirStmt::Expr(ue));
            loop_body.push(TirStmt::If {
                cond: TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Not,
                        operand: Box::new(first_var()),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: Span::EMPTY,
                },
                then_body: upd,
                else_body: vec![],
            });
        }
        loop_body.push(TirStmt::Expr(TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(first_var()),
                value: Box::new(bool_lit(false)),
            },
            ty: BackendTy::Void,
            res: Resolution::None,
            span: Span::EMPTY,
        }));

        if let Some(t) = test {
            let cond = self.lower_cond(t);
            loop_body.extend(std::mem::take(&mut self.pending));
            loop_body.push(TirStmt::If {
                cond,
                then_body: vec![],
                else_body: vec![TirStmt::Break],
            });
        }

        loop_body.extend(self.lower_stmt_as_block(body));
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
