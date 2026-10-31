use super::context::FnEmitter;
use super::small_utils::bool_lit;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_await(&mut self, argument: ExprId, ty: BackendTy, span: Span) -> TirExpr {
        if self.top_level {
            self.saw_await = true;
        }
        let fut = self.lower_expr(argument);
        TirExpr {
            kind: TirExprKind::Await {
                future: Box::new(fut),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_yield(
        &mut self,
        argument: Option<ExprId>,
        delegate: bool,
        span: Span,
    ) -> TirExpr {
        let value = argument.map(|a| {
            let value = self.lower_expr(a);
            Box::new(if delegate {
                value
            } else {
                let value = self.widened(value, self.yield_ty);
                self.guard_dynamic(value, self.yield_ty)
            })
        });
        TirExpr {
            kind: TirExprKind::Yield { value, delegate },
            ty: BackendTy::Dynamic(DynReason::Declared),
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_yield_stmt(
        &mut self,
        argument: Option<ExprId>,
        span: Span,
    ) -> Vec<TirStmt> {
        let dyn_ty = BackendTy::Dynamic(DynReason::Declared);
        let yield_expr = self.lower_yield(argument, false, span);
        let mut out = std::mem::take(&mut self.pending);
        let resumed = self.fresh_local(dyn_ty);
        out.push(TirStmt::Let {
            local: resumed,
            ty: dyn_ty,
            init: Some(yield_expr),
        });
        out.push(TirStmt::If {
            cond: TirExpr {
                kind: TirExprKind::Binary {
                    op: TirBinOp::Eq,
                    lhs: Box::new(TirExpr {
                        kind: TirExprKind::Var,
                        ty: dyn_ty,
                        res: Resolution::Local(resumed),
                        span,
                    }),
                    rhs: Box::new(bool_lit(true)),
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            },
            then_body: vec![TirStmt::Return(None)],
            else_body: vec![],
        });
        out
    }
}
