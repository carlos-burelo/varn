use super::context::FnEmitter;
use super::small_utils::{bool_lit, int_lit};
use std::sync::Arc;
use varn_core::ast::{ExprId, ExprKind, Pattern, StmtId};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_for_of(
        &mut self,
        left: &Pattern,
        right: ExprId,
        body: StmtId,
        is_await: bool,
    ) -> Vec<TirStmt> {
        let arena = self.ast_arena;
        let int_range = match &arena.expr(right).kind {
            ExprKind::Range { start, end, .. } => {
                matches!(self.expr_ty(*start), BackendTy::Int)
                    && matches!(self.expr_ty(*end), BackendTy::Int)
            }
            _ => false,
        };
        if !is_await && int_range {
            if let (
                ExprKind::Range {
                    start,
                    end,
                    inclusive,
                },
                Pattern::Identifier { name, .. },
            ) = (&self.ast_arena.expr(right).kind, left)
            {
                let (start, end, inclusive) = (*start, *end, *inclusive);
                let name: Arc<str> = Arc::from(self.m.interner.resolve(*name));
                let lo = self.lower_expr(start);
                let mut hi = self.lower_expr(end);
                if inclusive {
                    hi = TirExpr {
                        kind: TirExprKind::Binary {
                            op: TirBinOp::Add,
                            lhs: Box::new(hi),
                            rhs: Box::new(int_lit(1)),
                        },
                        ty: BackendTy::Int,
                        res: Resolution::None,
                        span: Span::EMPTY,
                    };
                }
                let mut out = std::mem::take(&mut self.pending);
                let hi = self.hoist(hi);
                out.extend(std::mem::take(&mut self.pending));
                let i = self.bind_local(name, BackendTy::Int);
                let ivar = || TirExpr {
                    kind: TirExprKind::Var,
                    ty: BackendTy::Int,
                    res: Resolution::Local(i),
                    span: Span::EMPTY,
                };

                let start = TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Sub,
                        lhs: Box::new(lo),
                        rhs: Box::new(int_lit(1)),
                    },
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span: Span::EMPTY,
                };
                out.push(TirStmt::Let {
                    local: i,
                    ty: BackendTy::Int,
                    init: Some(start),
                });
                let cond = TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Lt,
                        lhs: Box::new(ivar()),
                        rhs: Box::new(hi),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: Span::EMPTY,
                };
                let step = TirStmt::Expr(TirExpr {
                    kind: TirExprKind::Assign {
                        target: Box::new(ivar()),
                        value: Box::new(TirExpr {
                            kind: TirExprKind::Binary {
                                op: TirBinOp::Add,
                                lhs: Box::new(ivar()),
                                rhs: Box::new(int_lit(1)),
                            },
                            ty: BackendTy::Int,
                            res: Resolution::None,
                            span: Span::EMPTY,
                        }),
                    },
                    ty: BackendTy::Void,
                    res: Resolution::None,
                    span: Span::EMPTY,
                });
                let mut loop_body = vec![
                    step,
                    TirStmt::If {
                        cond,
                        then_body: vec![],
                        else_body: vec![TirStmt::Break],
                    },
                ];
                loop_body.extend(self.lower_stmt_as_block(body));
                out.push(TirStmt::Loop {
                    cond: bool_lit(true),
                    body: loop_body,
                });
                return out;
            }
        }

        let iter = self.lower_expr(right);
        let mut out = std::mem::take(&mut self.pending);

        if is_await {
            return self.lower_for_of_protocol(left, iter, out, body, true);
        }

        let Pattern::Identifier { name, .. } = left else {
            return self.lower_for_of_protocol(left, iter, out, body, false);
        };

        let elem_ty = match iter.ty.non_nullable(self.tt) {
            BackendTy::Array(el) => self.tt.get(el),
            _ => return self.lower_for_of_protocol(left, iter, out, body, false),
        };
        let arr = self.hoist(iter);
        out.extend(std::mem::take(&mut self.pending));
        let name: Arc<str> = Arc::from(self.m.interner.resolve(*name));
        out.extend(self.for_of_over_array(&name, arr, elem_ty, body));
        out
    }

    pub(super) fn for_of_over_array(
        &mut self,
        name: &Arc<str>,
        arr: TirExpr,
        elem_ty: BackendTy,
        body: StmtId,
    ) -> Vec<TirStmt> {
        let mut out = Vec::new();
        let idx = self.fresh_local(BackendTy::Int);

        out.push(TirStmt::Let {
            local: idx,
            ty: BackendTy::Int,
            init: Some(int_lit(-1)),
        });
        let idx_var = || TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Int,
            res: Resolution::Local(idx),
            span: Span::EMPTY,
        };

        let len = self.field_access(
            arr.clone(),
            Arc::from("length"),
            BackendTy::Int,
            Span::EMPTY,
        );
        let cond = TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Lt,
                lhs: Box::new(idx_var()),
                rhs: Box::new(len),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span: Span::EMPTY,
        };

        let elem = TirExpr {
            kind: TirExprKind::Index {
                object: Box::new(arr),
                index: Box::new(idx_var()),
            },
            ty: elem_ty,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        let x_local = self.bind_local(name.clone(), elem_ty);

        let step = TirStmt::Expr(TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(idx_var()),
                value: Box::new(TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Add,
                        lhs: Box::new(idx_var()),
                        rhs: Box::new(int_lit(1)),
                    },
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span: Span::EMPTY,
                }),
            },
            ty: BackendTy::Void,
            res: Resolution::None,
            span: Span::EMPTY,
        });
        let mut loop_body = vec![
            step,
            TirStmt::If {
                cond,
                then_body: vec![],
                else_body: vec![TirStmt::Break],
            },
            TirStmt::Let {
                local: x_local,
                ty: elem_ty,
                init: Some(elem),
            },
        ];
        loop_body.extend(self.lower_stmt_as_block(body));

        out.push(TirStmt::Loop {
            cond: bool_lit(true),
            body: loop_body,
        });
        out
    }

    pub(super) fn lower_for_of_protocol(
        &mut self,
        left: &Pattern,
        src: TirExpr,
        mut out: Vec<TirStmt>,
        body: StmtId,
        is_await: bool,
    ) -> Vec<TirStmt> {
        let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
        let by_name = |n: &str| Resolution::ByName {
            name: Arc::from(n),
            why: DynReason::Unannotated,
        };

        out.extend(std::mem::take(&mut self.pending));

        let iter_getter = TirExpr {
            kind: TirExprKind::IterInit {
                source: Box::new(src),
                is_async: is_await,
            },
            ty: dyn_ty,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        let it = self.fresh_local(dyn_ty);
        out.push(TirStmt::Let {
            local: it,
            ty: dyn_ty,
            init: Some(iter_getter),
        });
        let it_var = || TirExpr {
            kind: TirExprKind::Var,
            ty: dyn_ty,
            res: Resolution::Local(it),
            span: Span::EMPTY,
        };

        let step = self.fresh_local(dyn_ty);
        let step_var = || TirExpr {
            kind: TirExprKind::Var,
            ty: dyn_ty,
            res: Resolution::Local(step),
            span: Span::EMPTY,
        };
        let field = |recv: TirExpr, n: &str| TirExpr {
            kind: TirExprKind::Field {
                object: Box::new(recv),
                name: Arc::from(n),
            },
            ty: BackendTy::Dynamic(DynReason::Unannotated),
            res: Resolution::ByName {
                name: Arc::from(n),
                why: DynReason::Unannotated,
            },
            span: Span::EMPTY,
        };

        let mut next_call = TirExpr {
            kind: TirExprKind::MethodCall {
                recv: Box::new(it_var()),
                name: Arc::from("next"),
                args: vec![],
            },
            ty: dyn_ty,
            res: by_name("next"),
            span: Span::EMPTY,
        };
        if is_await {
            next_call = TirExpr {
                kind: TirExprKind::Await {
                    future: Box::new(next_call),
                },
                ty: dyn_ty,
                res: Resolution::None,
                span: Span::EMPTY,
            };
        }
        let mut loop_body = vec![
            TirStmt::Let {
                local: step,
                ty: dyn_ty,
                init: Some(next_call),
            },
            TirStmt::If {
                cond: field(step_var(), "done"),
                then_body: vec![TirStmt::Break],
                else_body: vec![],
            },
        ];
        let value = field(step_var(), "value");
        self.bind_pattern(left, value, &mut loop_body);
        loop_body.extend(self.lower_stmt_as_block(body));

        out.push(TirStmt::Loop {
            cond: bool_lit(true),
            body: loop_body,
        });
        out
    }
}
