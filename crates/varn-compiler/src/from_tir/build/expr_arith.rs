use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use varn_tir::{TirBinOp, TirExpr, TirUnOp};

impl<'m> Builder<'m> {
    pub(super) fn lower_binary(
        &mut self,
        op: TirBinOp,
        lhs: &TirExpr,
        rhs: &TirExpr,
        ty: HirType,
    ) -> Result<Value> {
        let mut l = self.lower_expr(lhs)?;
        let mut r = self.lower_expr(rhs)?;
        let is_cmp = matches!(
            op,
            TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
        );
        if !is_cmp && matches!(ty, HirType::Int | HirType::Float | HirType::Str) {
            l = self.coerce(l, ty);
            r = self.coerce(r, ty);
        }
        let (lt, rt) = (self.value_ty(l), self.value_ty(r));
        let op_ty = if lt == rt
            && matches!(
                lt,
                HirType::Int | HirType::Float | HirType::Bool | HirType::Str
            ) {
            lt
        } else {
            HirType::Dynamic
        };
        let result_ty = if is_cmp && op_ty != HirType::Dynamic {
            HirType::Bool
        } else if op_ty != HirType::Dynamic {
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
        ty: HirType,
    ) -> Result<Value> {
        let v = self.lower_expr(operand)?;
        match op {
            TirUnOp::IsNull => Ok(self.emit(InstKind::IsNull { operand: v }, HirType::Bool)),
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

    pub(super) fn lower_cast(
        &mut self,
        operand: &TirExpr,
        node_ty: varn_tir::BackendTy,
        ty: HirType,
    ) -> Result<Value> {
        use varn_core::NumericDomain as D;
        let v = self.lower_expr(operand)?;
        let from = match self.value_ty(v) {
            HirType::Int => Some(D::Int),
            HirType::Float => Some(D::Float),
            HirType::Bool
            | HirType::Str
            | HirType::Ref
            | HirType::Dynamic
            | HirType::Array(_)
            | HirType::Map(..)
            | HirType::Set(_)
            | HirType::Class(_)
            | HirType::Nullable(_) => super::ops::numeric_domain(operand.ty)
                .filter(|d| matches!(d, D::BigInt | D::Decimal)),
        };
        let to = super::ops::numeric_domain(node_ty);
        let conv = match (from, to) {
            (Some(f), Some(t)) if f == t => return Ok(v),
            (Some(f), Some(t)) => varn_core::NumConv::between(f, t),
            (None, Some(D::Int)) => Some(varn_core::NumConv::DynToInt),
            (None, Some(D::Float)) => Some(varn_core::NumConv::DynToFloat),
            _ => None,
        };
        if let Some(conv) = conv {
            let result_ty = crate::ssa::verify::convert_result_ty(conv);
            return Ok(self.emit(InstKind::Convert { operand: v, conv }, result_ty));
        }
        Ok(self.emit(InstKind::Cast { operand: v, ty }, ty))
    }
}
