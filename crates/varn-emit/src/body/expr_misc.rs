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
            | BackendTy::Dict(_)
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
        let BackendTy::Dynamic(reason) = e.ty else {
            return false;
        };
        let untyped = untyped_reason(reason);
        match ty.non_nullable(self.tt) {
            BackendTy::Str
            | BackendTy::Bool
            | BackendTy::Int
            | BackendTy::Float
            | BackendTy::Class(_) => true,
            BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Dynamic(varn_tir::DynReason::Callable | varn_tir::DynReason::Shape(_)) => {
                untyped || matches!(reason, varn_tir::DynReason::NotYetSupported)
            }
            BackendTy::Dynamic(
                varn_tir::DynReason::HostBoundary
                | varn_tir::DynReason::Union
                | varn_tir::DynReason::IndexSignature
                | varn_tir::DynReason::Declared
                | varn_tir::DynReason::Awaitable(_)
                | varn_tir::DynReason::Yields(_)
                | varn_tir::DynReason::NotYetSupported,
            )
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never => false,
        }
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

pub(super) fn untyped_reason(reason: varn_tir::DynReason) -> bool {
    match reason {
        varn_tir::DynReason::Declared | varn_tir::DynReason::HostBoundary => true,
        varn_tir::DynReason::Union
        | varn_tir::DynReason::IndexSignature
        | varn_tir::DynReason::Callable
        | varn_tir::DynReason::Awaitable(_)
        | varn_tir::DynReason::Yields(_)
        | varn_tir::DynReason::Shape(_)
        | varn_tir::DynReason::NotYetSupported => false,
    }
}
