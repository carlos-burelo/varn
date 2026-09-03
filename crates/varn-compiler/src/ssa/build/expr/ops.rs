use std::rc::Rc;

use crate::hir::{HirExpr, HirLogicalOp, HirTemplatePart, HirType};
use crate::ssa::ir::{InstKind, Value};

use super::{Builder, Result};

impl Builder {
    /// Operators: logical short-circuits, the conditional, binary and unary
    /// arithmetic, and template concatenation.
    pub(super) fn lower_operator_expr(&mut self, expr: &HirExpr) -> Result<Value> {
        match expr {
            HirExpr::Logical { op, lhs, rhs, ty } => {
                let l = self.lower_expr(lhs)?;
                match op {
                    HirLogicalOp::And => {
                        self.lower_branch_value(l, |s| s.lower_expr(rhs), |_| Ok(l), *ty)
                    }

                    HirLogicalOp::Or => {
                        self.lower_branch_value(l, |_| Ok(l), |s| s.lower_expr(rhs), *ty)
                    }

                    HirLogicalOp::Nullish => {
                        let isnull = self.emit(InstKind::IsNull { operand: l }, HirType::Bool);
                        self.lower_branch_value(isnull, |s| s.lower_expr(rhs), |_| Ok(l), *ty)
                    }
                }
            }

            HirExpr::Template(parts) => {
                let mut pvals = Vec::with_capacity(parts.len());
                for part in parts {
                    match part {
                        HirTemplatePart::Str(s) => {
                            pvals.push(self.emit(InstKind::ConstStr(s.clone()), HirType::Str))
                        }
                        HirTemplatePart::Expr(e) => {
                            let v = self.lower_expr(e)?;
                            pvals.push(self.emit(InstKind::ToString { operand: v }, HirType::Str));
                        }
                    }
                }
                if pvals.is_empty() {
                    Ok(self.emit(InstKind::ConstStr(Rc::from("")), HirType::Str))
                } else {
                    Ok(self.emit(InstKind::BuildStr { parts: pvals }, HirType::Str))
                }
            }
            HirExpr::Conditional {
                test,
                cons,
                alt,
                ty,
            } => {
                let t = self.lower_expr(test)?;
                self.lower_branch_value(t, |s| s.lower_expr(cons), |s| s.lower_expr(alt), *ty)
            }
            HirExpr::Binary { op, lhs, rhs, ty } => {
                let l = self.lower_expr(lhs)?;
                let r = self.lower_expr(rhs)?;
                let effective_ty = if *ty != HirType::Dynamic
                    && self.value_ty(l) == *ty
                    && self.value_ty(r) == *ty
                {
                    *ty
                } else {
                    HirType::Dynamic
                };
                // `effective_ty` is the operand class (drives opcode selection); the
                // value's type is the result class — they differ for
                // `int / int → float` and for comparisons.
                let result_ty = crate::hir::binary_result_ty(*op, effective_ty);
                // `+` with one proven `str` operand is concatenation, and its
                // result is a `str`. The operand class cannot say so: for
                // `"n=" + count` the two sides are different types, so the
                // checker records no numeric class and `ty` is `Dynamic`.
                //
                // Deliberately the SAME condition `ssa::emit::values` uses to
                // pick `StrConcat` over the generic add, read from the same
                // proven operand types at the same point in the pipeline — so
                // the value's type and the opcode cannot disagree about what
                // the instruction produces.
                let result_ty = if matches!(op, crate::hir::HirBinOp::Add)
                    && result_ty == HirType::Dynamic
                    && (self.value_ty(l) == HirType::Str || self.value_ty(r) == HirType::Str)
                {
                    HirType::Str
                } else {
                    result_ty
                };
                Ok(self.emit(
                    InstKind::Binary {
                        op: *op,
                        lhs: l,
                        rhs: r,
                        ty: effective_ty,
                    },
                    result_ty,
                ))
            }
            HirExpr::Unary { op, operand, ty } => {
                let o = self.lower_expr(operand)?;
                let effective_ty = if *ty != HirType::Dynamic && self.value_ty(o) == *ty {
                    *ty
                } else {
                    HirType::Dynamic
                };
                let result_ty = match op {
                    crate::hir::HirUnOp::Not => HirType::Bool,
                    crate::hir::HirUnOp::Typeof => HirType::Str,
                    crate::hir::HirUnOp::Neg | crate::hir::HirUnOp::BitNot => effective_ty,
                };
                Ok(self.emit(
                    InstKind::Unary {
                        op: *op,
                        operand: o,
                        ty: effective_ty,
                    },
                    result_ty,
                ))
            }

            other => unreachable!("lower_operator_expr: {other:?} is not handled here"),
        }
    }
}
