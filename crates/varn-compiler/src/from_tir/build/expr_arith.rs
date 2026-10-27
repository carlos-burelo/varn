use super::context::{Builder, Result};
use crate::ssa::ir::{InstKind, Value};
use varn_tir::{BackendTy, DynReason};
use varn_tir::{TirBinOp, TirExpr, TirUnOp};

impl<'m> Builder<'m> {
    pub(super) fn lower_binary(
        &mut self,
        op: TirBinOp,
        lhs: &TirExpr,
        rhs: &TirExpr,
        ty: BackendTy,
    ) -> Result<Value> {
        let mut l = self.lower_expr(lhs)?;
        let mut r = self.lower_expr(rhs)?;
        let is_cmp = matches!(
            op,
            TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
        );
        if !is_cmp && matches!(ty, BackendTy::Int | BackendTy::Float | BackendTy::Str) {
            l = self.coerce(l, ty);
            r = self.coerce(r, ty);
        }
        let (lt, rt) = (self.value_ty(l), self.value_ty(r));
        let op_ty = if lt == rt
            && matches!(
                lt,
                BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Str
            ) {
            lt
        } else {
            BackendTy::Dynamic(DynReason::Boxed)
        };
        let result_ty = if is_cmp && op_ty != BackendTy::Dynamic(DynReason::Boxed) {
            BackendTy::Bool
        } else if op_ty != BackendTy::Dynamic(DynReason::Boxed) {
            op_ty
        } else {
            ty
        };
        Ok(self.emit(
            InstKind::Binary {
                op: super::ops::bin_op(op),
                lhs: l,
                rhs: r,
                ty: op_ty,
            },
            result_ty,
        ))
    }

    pub(super) fn lower_unary(
        &mut self,
        op: TirUnOp,
        operand: &TirExpr,
        ty: BackendTy,
    ) -> Result<Value> {
        let v = self.lower_expr(operand)?;
        match op {
            TirUnOp::IsNull => match self.value_ty(v) {
                BackendTy::Int | BackendTy::Float | BackendTy::Bool => {
                    Ok(self.emit(InstKind::ConstBool(false), BackendTy::Bool))
                }
                BackendTy::Str
                | BackendTy::Char
                | BackendTy::Bytes
                | BackendTy::Enum(_)
                | BackendTy::Fn(_)
                | BackendTy::Dynamic(_)
                | BackendTy::Decimal
                | BackendTy::BigInt
                | BackendTy::Void
                | BackendTy::Never
                | BackendTy::Array(_)
                | BackendTy::Map(..)
                | BackendTy::Dict(_)
                | BackendTy::Set(_)
                | BackendTy::Tuple(_)
                | BackendTy::Class(_)
                | BackendTy::Nullable(_) => {
                    Ok(self.emit(InstKind::IsNull { operand: v }, BackendTy::Bool))
                }
            },
            TirUnOp::Present => {
                self.emit_effect(InstKind::AssertNotNull { operand: v });
                Ok(v)
            }
            TirUnOp::ToStr => Ok(self.emit(
                InstKind::Cast {
                    operand: v,
                    ty: BackendTy::Str,
                },
                BackendTy::Str,
            )),
            TirUnOp::Neg | TirUnOp::Not | TirUnOp::BitNot | TirUnOp::Typeof => {
                let result = self.emit(
                    InstKind::Unary {
                        op: super::ops::un_op(op),
                        operand: v,
                        ty,
                    },
                    ty,
                );
                Ok(result)
            }
        }
    }

    fn static_truthiness(&mut self, v: Value) -> Option<Value> {
        match self.value_ty(v) {
            BackendTy::Bool => Some(v),
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Str
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_) => Some(self.emit(InstKind::ConstBool(true), BackendTy::Bool)),
            BackendTy::Nullable(inner) => match self.tir.types.get(inner) {
                BackendTy::Int
                | BackendTy::Float
                | BackendTy::Str
                | BackendTy::Array(_)
                | BackendTy::Map(..)
                | BackendTy::Dict(_)
                | BackendTy::Set(_)
                | BackendTy::Tuple(_)
                | BackendTy::Class(_) => {
                    let absent = self.emit(InstKind::IsNull { operand: v }, BackendTy::Bool);
                    Some(self.emit(
                        InstKind::Unary {
                            op: crate::hir::HirUnOp::Not,
                            operand: absent,
                            ty: BackendTy::Bool,
                        },
                        BackendTy::Bool,
                    ))
                }
                BackendTy::Bool
                | BackendTy::Dynamic(_)
                | BackendTy::Char
                | BackendTy::Bytes
                | BackendTy::Enum(_)
                | BackendTy::Fn(_)
                | BackendTy::Decimal
                | BackendTy::BigInt
                | BackendTy::Void
                | BackendTy::Never
                | BackendTy::Nullable(_) => None,
            },
            BackendTy::Dynamic(_)
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Void
            | BackendTy::Never => None,
        }
    }

    pub(super) fn lower_cast(
        &mut self,
        operand: &TirExpr,
        node_ty: varn_tir::BackendTy,
        ty: BackendTy,
    ) -> Result<Value> {
        use varn_core::NumericDomain as D;
        let v = self.lower_expr(operand)?;
        if ty == BackendTy::Bool
            && let Some(holds) = self.static_truthiness(v)
        {
            return Ok(holds);
        }
        let from = match self.value_ty(v) {
            BackendTy::Int => Some(D::Int),
            BackendTy::Float => Some(D::Float),
            BackendTy::Bool
            | BackendTy::Str
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Dynamic(_)
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Nullable(_) => super::ops::numeric_domain(operand.ty)
                .filter(|d| matches!(d, D::BigInt | D::Decimal)),
        };
        let to = super::ops::numeric_domain(node_ty);
        let conv = match (from, to) {
            (Some(f), Some(t)) if f == t => return Ok(v),
            (Some(f), Some(t)) => varn_core::ValueConv::between(f, t),
            (None, Some(D::Int)) => Some(varn_core::ValueConv::DynToInt),
            (None, Some(D::Float)) => Some(varn_core::ValueConv::DynToFloat),
            _ => None,
        };
        if let Some(conv) = conv {
            let result_ty = crate::ssa::verify::convert_result_ty(conv);
            return Ok(self.emit(InstKind::Convert { operand: v, conv }, result_ty));
        }
        Ok(self.emit(InstKind::Cast { operand: v, ty }, ty))
    }
}
