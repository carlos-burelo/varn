use std::rc::Rc;

use crate::hir::{HirExpr, HirLogicalOp, HirTemplatePart, HirType};
use crate::ssa::ir::{InstKind, Value};

use super::{Builder, Result};


impl Builder {
    /// Operators: logical short-circuits, the conditional, binary and unary
    /// arithmetic, and template concatenation.
    pub(super) fn lower_operator_expr(&mut self, expr: &HirExpr) -> Result<Value> {
        match expr {
            HirExpr::Logical { op, lhs, rhs } => {
                let l = self.lower_expr(lhs)?;
                match op {
                    HirLogicalOp::And => self.lower_branch_value(
                        l,
                        |s| s.lower_expr(rhs),
                        |_| Ok(l),
                        HirType::Dynamic,
                    ),

                    HirLogicalOp::Or => self.lower_branch_value(
                        l,
                        |_| Ok(l),
                        |s| s.lower_expr(rhs),
                        HirType::Dynamic,
                    ),

                    HirLogicalOp::Nullish => {
                        let isnull = self.emit(InstKind::IsNull { operand: l }, HirType::Bool);
                        self.lower_branch_value(
                            isnull,
                            |s| s.lower_expr(rhs),
                            |_| Ok(l),
                            HirType::Dynamic,
                        )
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
            HirExpr::Conditional { test, cons, alt } => {
                let t = self.lower_expr(test)?;
                self.lower_branch_value(
                    t,
                    |s| s.lower_expr(cons),
                    |s| s.lower_expr(alt),
                    HirType::Dynamic,
                )
            }
            HirExpr::Binary { op, lhs, rhs, ty } => {
                let l = self.lower_expr(lhs)?;
                let r = self.lower_expr(rhs)?;
                // `ty` is the operand class (drives opcode selection); the
                // value's type is the result class — they differ for
                // `int / int → float` and for comparisons.
                let result_ty = crate::hir::binary_result_ty(*op, *ty);
                Ok(self.emit(
                    InstKind::Binary {
                        op: *op,
                        lhs: l,
                        rhs: r,
                        ty: *ty,
                    },
                    result_ty,
                ))
            }
            HirExpr::Unary { op, operand, ty } => {
                let o = self.lower_expr(operand)?;
                Ok(self.emit(
                    InstKind::Unary {
                        op: *op,
                        operand: o,
                        ty: *ty,
                    },
                    *ty,
                ))
            }

            other => unreachable!("lower_operator_expr: {other:?} is not handled here"),
        }
    }
}
