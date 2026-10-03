use super::context::FnEmitter;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirExpr, TirExprKind};

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
        let value = argument.map(|a| Box::new(self.lower_expr(a)));
        TirExpr {
            kind: TirExprKind::Yield { value, delegate },

            ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            res: Resolution::None,
            span,
        }
    }
}
