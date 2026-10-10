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
            TirUnOp::ToStr => Ok(self.emit(
                InstKind::Cast {
                    operand: v,
                    ty: HirType::Str,
                },
                HirType::Str,
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

    fn lower_checked_class(
        &mut self,
        v: Value,
        class: varn_tir::ClassId,
        nullable: bool,
        ty: HirType,
    ) -> Result<Value> {
        let name = self
            .tir
            .class(class)
            .map(|ci| ci.name.clone())
            .ok_or(crate::OptError::Unsupported("from_tir: Checked to an unknown class"))?;
        let cls = self.emit(self.global_load(&name), HirType::Ref);
        let is_instance = self.emit(
            InstKind::Binary {
                op: crate::hir::HirBinOp::Instanceof,
                lhs: v,
                rhs: cls,
                ty: HirType::Dynamic,
            },
            HirType::Bool,
        );
        let holds = if nullable {
            let is_null = self.emit(InstKind::IsNull { operand: v }, HirType::Bool);
            let not_null = self.emit(
                InstKind::Unary {
                    op: crate::hir::HirUnOp::Not,
                    operand: is_null,
                    ty: HirType::Bool,
                },
                HirType::Bool,
            );
            self.emit(
                InstKind::Binary {
                    op: crate::hir::HirBinOp::Eq,
                    lhs: is_instance,
                    rhs: not_null,
                    ty: HirType::Bool,
                },
                HirType::Bool,
            )
        } else {
            is_instance
        };
        self.emit(
            InstKind::Convert {
                operand: holds,
                conv: varn_core::ValueConv::AssertInstance,
            },
            HirType::Bool,
        );
        Ok(self.emit(InstKind::Cast { operand: v, ty }, ty))
    }

    pub(super) fn lower_checked(
        &mut self,
        operand: &TirExpr,
        node_ty: varn_tir::BackendTy,
        ty: HirType,
    ) -> Result<Value> {
        let v = self.lower_expr(operand)?;
        match node_ty {
            varn_tir::BackendTy::Class(class) => {
                return self.lower_checked_class(v, class, false, ty);
            }
            varn_tir::BackendTy::Nullable(inner) if self.tir.types.contains(inner) => {
                if let varn_tir::BackendTy::Class(class) = self.tir.types.get(inner) {
                    return self.lower_checked_class(v, class, true, ty);
                }
            }
            varn_tir::BackendTy::Int
            | varn_tir::BackendTy::Float
            | varn_tir::BackendTy::Bool
            | varn_tir::BackendTy::Char
            | varn_tir::BackendTy::Str
            | varn_tir::BackendTy::Bytes
            | varn_tir::BackendTy::Decimal
            | varn_tir::BackendTy::BigInt
            | varn_tir::BackendTy::Array(_)
            | varn_tir::BackendTy::Map(..)
            | varn_tir::BackendTy::Set(_)
            | varn_tir::BackendTy::Tuple(_)
            | varn_tir::BackendTy::Enum(_)
            | varn_tir::BackendTy::Fn(_)
            | varn_tir::BackendTy::Nullable(_)
            | varn_tir::BackendTy::Void
            | varn_tir::BackendTy::Never
            | varn_tir::BackendTy::Dynamic(_) => {}
        }
        let conv = match ty {
            HirType::Str => varn_core::ValueConv::ExpectStr,
            HirType::Bool => varn_core::ValueConv::ExpectBool,
            HirType::Int => varn_core::ValueConv::DynToInt,
            HirType::Float => varn_core::ValueConv::DynToFloat,
            HirType::Nullable(inner) => {
                let conv = match self.ssa_types.get(inner) {
                    HirType::Str => varn_core::ValueConv::ExpectStrOrNull,
                    HirType::Bool => varn_core::ValueConv::ExpectBoolOrNull,
                    HirType::Int
                    | HirType::Float
                    | HirType::Ref
                    | HirType::Dynamic
                    | HirType::Array(_)
                    | HirType::Map(..)
                    | HirType::Set(_)
                    | HirType::Class(_)
                    | HirType::Nullable(_) => {
                        return Err(crate::OptError::Unsupported(
                            "from_tir: Checked to an unsupported nullable type",
                        ));
                    }
                };
                let checked = self.emit(InstKind::Convert { operand: v, conv }, HirType::Dynamic);
                return Ok(self.emit(InstKind::Cast { operand: checked, ty }, ty));
            }
            HirType::Ref
            | HirType::Dynamic
            | HirType::Array(_)
            | HirType::Map(..)
            | HirType::Set(_)
            | HirType::Class(_) => {
                return Err(crate::OptError::Unsupported("from_tir: Checked to a non-scalar type"));
            }
        };
        let result_ty = crate::ssa::verify::convert_result_ty(conv);
        Ok(self.emit(InstKind::Convert { operand: v, conv }, result_ty))
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
