use super::context::FnEmitter;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, Resolution, Span, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_as_cast(
        &mut self,
        expression: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let inner = self.lower_expr(expression);
        if matches!(inner.ty, BackendTy::Enum(_)) && matches!(ty, BackendTy::Int) {
            return self.field_access(inner, Arc::from("rawValue"), ty, span);
        }
        TirExpr {
            kind: TirExprKind::Cast {
                operand: Box::new(inner),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_sequence(&mut self, expressions: &[ExprId], span: Span) -> TirExpr {
        let Some((&last, lead)) = expressions.split_last() else {
            return TirExpr {
                kind: TirExprKind::NullLit,
                ty: BackendTy::Void,
                res: Resolution::None,
                span,
            };
        };
        for &e in lead {
            let te = self.lower_expr(e);
            self.pending.push(TirStmt::Expr(te));
        }
        self.lower_expr(last)
    }

    pub(super) fn lower_conditional(
        &mut self,
        test: ExprId,
        consequent: ExprId,
        alternate: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let cond = self.lower_expr(test);
        let cond = self.cast_to(cond, BackendTy::Bool);
        let then_val = self.lower_expr(consequent);
        let else_val = self.lower_expr(alternate);
        let (then_val, else_val) =
            if then_val.ty == else_val.ty || matches!(ty, BackendTy::Dynamic(_)) {
                (then_val, else_val)
            } else {
                (self.cast_to(then_val, ty), self.cast_to(else_val, ty))
            };
        TirExpr {
            kind: TirExprKind::Select {
                cond: Box::new(cond),
                then_val: Box::new(then_val),
                else_val: Box::new(else_val),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }
}
