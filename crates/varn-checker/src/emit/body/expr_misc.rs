use super::context::FnEmitter;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_cond(&mut self, e: ExprId) -> TirExpr {
        let lowered = self.lower_expr(e);
        match lowered.ty {
            BackendTy::Bool | BackendTy::Dynamic(_) => lowered,
            BackendTy::Int | BackendTy::Float | BackendTy::Char | BackendTy::Str | BackendTy::Bytes | BackendTy::Decimal | BackendTy::BigInt | BackendTy::Array(_) | BackendTy::Map(..) | BackendTy::Set(_) | BackendTy::Tuple(_) | BackendTy::Class(_) | BackendTy::Enum(_) | BackendTy::Fn(_) | BackendTy::Nullable(_) | BackendTy::Void | BackendTy::Never => self.cast_to(lowered, BackendTy::Bool),
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
