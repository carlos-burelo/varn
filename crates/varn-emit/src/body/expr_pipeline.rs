use super::call_contract::Callee;
use super::context::FnEmitter;
use varn_core::ast::{Arg, AstId, ExprId, ExprKind};
use varn_tir::{BackendTy, Resolution, Span, TirArg, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_pipeline(
        &mut self,
        left: ExprId,
        right: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        if let ExprKind::Call { callee, args, .. } = &self.ast_arena.expr(right).kind {
            let (callee, args) = (*callee, args);
            let interner = self.m.interner;
            let has_placeholder = args.iter().any(|a| {
                matches!(
                    a,
                    Arg::Positional(e) | Arg::Named { value: e, .. }
                        if matches!(&self.ast_arena.expr(*e).kind, ExprKind::Identifier { name } if interner.resolve(*name) == varn_core::TokenKind::Placeholder.as_str())
                )
            });
            if has_placeholder {
                let lv = self.lower_expr(left);
                let piped = self.hoist(lv);
                let c = self.lower_expr(callee);
                let targs: Vec<TirArg> = args
                    .iter()
                    .map(|a| match a {
                        Arg::Positional(e)
                        | Arg::Named { value: e, .. }
                            if matches!(&self.ast_arena.expr(*e).kind, ExprKind::Identifier { name } if interner.resolve(*name) == varn_core::TokenKind::Placeholder.as_str()) =>
                        {
                            TirArg::Expr(piped.clone())
                        }
                        other @ Arg::Positional(_) | other @ Arg::Spread(_) | other @ Arg::Named { .. } => self.lower_arg(other),
                    })
                    .collect();
                return TirExpr {
                    kind: TirExprKind::Call {
                        callee: Box::new(c),
                        args: targs,
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
        }
        let arg = self.lower_expr(left);
        let callee = self.lower_expr(right);
        TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(callee),
                args: vec![TirArg::Expr(arg)],
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn by_name_call(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let c = self.lower_expr(callee);
        let targs = self.lower_call_args(call_id, args);
        let callee_sig = self.callee_signature(callee);
        let (target, targs) =
            self.settle_args(Some(call_id), Callee::Value(c), targs, callee_sig, span);
        let c = target.into_expr();
        TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(c),
                args: targs,
            },
            ty,
            res: Resolution::None,
            span,
        }
    }
}
