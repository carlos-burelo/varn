//! Operand validity of binary operators: numeric domains meet only through
//! `varn_core::numeric` (with exact literals adopting the other side),
//! strings concatenate and compare, bitwise operators take `int`.

use super::super::helpers::{base_type, op_str};
use super::super::infer::member_binary::normalize_for_binary;
use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::ExprId;
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl<'r> Checker<'r> {
    pub(super) fn check_binary_operands(
        &mut self,
        op: BinaryOp,
        left: ExprId,
        right: ExprId,
        range: SourceRange,
        bind: &BindResult,
    ) {
        let l_ty = self.infer_type(left, bind);
        let r_ty = self.infer_type(right, bind);

        let l_base_raw = base_type(&l_ty);
        let r_base_raw = base_type(&r_ty);
        let l_base = normalize_for_binary(&l_base_raw, &self.ty_table, &bind.interner);
        let r_base = normalize_for_binary(&r_base_raw, &self.ty_table, &bind.interner);
        let is_type_param_b = |t: &Type, checker: &Checker| matches!(checker.ty_table.get(t.0), varn_core::TypeKind::Named(n, _) if checker.active_type_params.contains(bind.interner.resolve(n)));
        if !l_base.is_dynamic()
            && !r_base.is_dynamic()
            && !is_type_param_b(&l_base, self)
            && !is_type_param_b(&r_base, self)
        {
            let is_numeric = |t: &Type, checker: &Checker| {
                t.is_numeric()
                    || matches!(checker.ty_table.get(t.0), TypeKind::Named(n, _) if bind.interner.resolve(n) == varn_core::LangPrimitive::Decimal.name())
            };
            let both_numeric = is_numeric(&l_base, self) && is_numeric(&r_base, self);
            let (l_eff, r_eff) = crate::binder::type_inference::adopt_literal_operands(
                self.ast_arena,
                left,
                right,
                l_base,
                r_base,
                &self.ty_table,
            );
            let same_numeric = both_numeric
                && crate::binder::type_inference::numeric_operands_compatible(
                    &l_eff,
                    &r_eff,
                    &self.ty_table,
                );
            let valid = match op {
                BinaryOp::Add => same_numeric || l_base == Type::Str || r_base == Type::Str,
                BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                    same_numeric
                }
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr => l_base == Type::Int && r_base == Type::Int,
                BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => {
                    same_numeric || (l_base == Type::Str && r_base == Type::Str)
                }
                BinaryOp::Eq | BinaryOp::NotEq => !both_numeric || same_numeric,
                _ => true,
            };
            if !valid {
                let l_ty_s = l_ty.display(&self.ty_table, &bind.interner);
                let r_ty_s = r_ty.display(&self.ty_table, &bind.interner);
                self.emit(
                    Diagnostic::error(
                        ErrorCode::InvalidTypeOperator,
                        format!(
                            "invalid binary operation '{}' between '{}' and '{}'",
                            op_str(&op),
                            l_ty_s,
                            r_ty_s
                        ),
                    )
                    .with_range(range),
                );
            }
        }
    }
}
