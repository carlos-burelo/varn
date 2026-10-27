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
            return self.field_access(
                inner,
                Arc::from(varn_core::MemberKey::RawValue.as_str()),
                ty,
                span,
            );
        }
        if inner.ty != ty && self.checks_structure(inner.ty, ty) {
            return self.check_structure(inner, ty);
        }
        self.assert_type(inner, ty)
    }

    fn checks_structure(&self, from: BackendTy, to: BackendTy) -> bool {
        let container = |t: BackendTy| {
            matches!(
                t.non_nullable(self.tt),
                BackendTy::Array(_)
                    | BackendTy::Map(..)
                    | BackendTy::Dict(_)
                    | BackendTy::Set(_)
                    | BackendTy::Tuple(_)
                    | BackendTy::Dynamic(
                        varn_tir::DynReason::Shape(_)
                            | varn_tir::DynReason::Record(_)
                            | varn_tir::DynReason::Interface(_)
                    )
            )
        };
        container(from) && container(to)
    }

    fn check_structure(&self, inner: TirExpr, ty: BackendTy) -> TirExpr {
        let span = inner.span;
        TirExpr {
            kind: TirExprKind::Checked {
                operand: Box::new(inner),
                kind: varn_tir::CheckKind::Conform,
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
