use super::context::FnEmitter;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_cond(&mut self, e: ExprId) -> TirExpr {
        let lowered = self.lower_expr(e);
        match lowered.ty {
            BackendTy::Bool | BackendTy::Dynamic(_) => lowered,
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never => self.cast_to(lowered, BackendTy::Bool),
        }
    }

    pub(super) fn to_str(&self, e: TirExpr) -> TirExpr {
        if e.ty == BackendTy::Str {
            return e;
        }
        let span = e.span;
        TirExpr {
            kind: TirExprKind::Unary {
                op: varn_tir::TirUnOp::ToStr,
                operand: Box::new(e),
            },
            ty: BackendTy::Str,
            res: varn_tir::Resolution::None,
            span,
        }
    }

    pub(super) fn assert_type(&self, e: TirExpr, ty: BackendTy) -> TirExpr {
        let numeric = matches!(ty.non_nullable(self.tt), BackendTy::Int | BackendTy::Float);
        if !numeric && self.crosses_dynamic_boundary(&e, ty) {
            self.guard_dynamic(e, ty)
        } else {
            self.cast_to(e, ty)
        }
    }

    fn crosses_dynamic_boundary(&self, e: &TirExpr, ty: BackendTy) -> bool {
        matches!(e.ty, BackendTy::Dynamic(_))
            && matches!(
                ty.non_nullable(self.tt),
                BackendTy::Str
                    | BackendTy::Bool
                    | BackendTy::Int
                    | BackendTy::Float
                    | BackendTy::Class(_)
            )
    }

    pub(super) fn guard_args(
        &self,
        args: Vec<varn_tir::TirArg>,
        sig: Option<varn_tir::SigId>,
    ) -> Vec<varn_tir::TirArg> {
        let Some(params) = sig
            .and_then(|s| self.signatures.get(s.0 as usize))
            .map(|s| s.params.clone())
        else {
            return args;
        };
        if args
            .iter()
            .any(|a| matches!(a, varn_tir::TirArg::Spread(_)))
        {
            return args;
        }
        args.into_iter()
            .enumerate()
            .map(|(i, arg)| match (arg, params.get(i)) {
                (varn_tir::TirArg::Expr(e), Some(&ty)) => {
                    varn_tir::TirArg::Expr(self.guard_dynamic(e, ty))
                }
                (other, _) => other,
            })
            .collect()
    }

    pub(super) fn guard_dynamic(&self, e: TirExpr, ty: BackendTy) -> TirExpr {
        if !self.crosses_dynamic_boundary(&e, ty) {
            return e;
        }
        let span = e.span;
        TirExpr {
            kind: TirExprKind::Checked {
                operand: Box::new(e),
            },
            ty,
            res: varn_tir::Resolution::None,
            span,
        }
    }

    pub(super) fn cast_to(&self, e: TirExpr, ty: BackendTy) -> TirExpr {
        if e.ty == ty {
            return e;
        }
        let span = e.span;
        TirExpr {
            kind: TirExprKind::Cast {
                operand: Box::new(e),
            },
            ty,
            res: varn_tir::Resolution::None,
            span,
        }
    }
}
