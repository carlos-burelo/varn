use super::context::FnEmitter;
use varn_tir::{BackendTy, Resolution, Span, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn keyed_read_ty(&mut self, container: BackendTy, slot: BackendTy) -> BackendTy {
        match container.non_nullable(self.tt) {
            BackendTy::Map(..) | BackendTy::Dict(_) => self.nullable_of(slot),
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => slot,
        }
    }

    pub(super) fn null_or(&mut self, is_null: TirExpr, value: TirExpr, span: Span) -> TirExpr {
        let result_ty = self.nullable_of(value.ty);
        let value = self.cast_to(value, result_ty);
        self.null_or_typed(is_null, value, result_ty, span)
    }

    pub(super) fn nullable_of(&mut self, ty: BackendTy) -> BackendTy {
        match ty {
            BackendTy::Nullable(_) | BackendTy::Dynamic(_) | BackendTy::Void => ty,
            member @ BackendTy::Int
            | member @ BackendTy::Float
            | member @ BackendTy::Bool
            | member @ BackendTy::Char
            | member @ BackendTy::Str
            | member @ BackendTy::Bytes
            | member @ BackendTy::Decimal
            | member @ BackendTy::BigInt
            | member @ BackendTy::Array(_)
            | member @ BackendTy::Map(..)
            | member @ BackendTy::Dict(_)
            | member @ BackendTy::Set(_)
            | member @ BackendTy::Tuple(_)
            | member @ BackendTy::Class(_)
            | member @ BackendTy::Enum(_)
            | member @ BackendTy::Fn(_)
            | member @ BackendTy::Never => BackendTy::Nullable(self.tt.intern(member)),
        }
    }

    pub(super) fn null_or_typed(
        &mut self,
        is_null: TirExpr,
        value: TirExpr,
        result_ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let null_arm = TirExpr {
            kind: TirExprKind::NullLit,
            ty: result_ty,
            res: Resolution::None,
            span,
        };
        TirExpr {
            kind: TirExprKind::Select {
                cond: Box::new(is_null),
                then_val: Box::new(null_arm),
                else_val: Box::new(value),
            },
            ty: result_ty,
            res: Resolution::None,
            span,
        }
    }
}
