use super::super::expr_labels::{base_type, op_str};
use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::ExprId;
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_binary_operands(
        &mut self,
        rec: &mut Recorder,
        op: BinaryOp,
        left: ExprId,
        right: ExprId,
        range: SourceRange,
        bind: &BindResult,
    ) {
        let l_ty = self.infer_type(rec, left, bind);
        let r_ty = self.infer_type(rec, right, bind);
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
            let is_null = |e: ExprId| {
                matches!(
                    self.ast_arena.expr(e).kind,
                    varn_core::ast::ExprKind::NullLiteral
                )
            };
            match (is_null(left), is_null(right)) {
                (false, true) => self.require_nullable(rec, left, "comparing it with null", bind),
                (true, false) => self.require_nullable(rec, right, "comparing it with null", bind),
                (true, true) | (false, false) => {}
            }
        }

        let l_base_raw = base_type(&l_ty);
        let r_base_raw = base_type(&r_ty);
        let l_base = l_base_raw.apparent(&self.ty_table);
        let r_base = r_base_raw.apparent(&self.ty_table);
        let is_type_param_b = |t: &Type, checker: &Checker| matches!(checker.ty_table.get(t.0), varn_core::TypeKind::Named(n, _) if checker.active_type_params.contains(bind.interner.try_resolve(n).unwrap_or_default()));
        if !l_base.is_dynamic()
            && !r_base.is_dynamic()
            && !is_type_param_b(&l_base, self)
            && !is_type_param_b(&r_base, self)
        {
            let both_numeric = l_base.is_numeric() && r_base.is_numeric();
            let (l_eff, r_eff) = varn_binder::type_inference::adopt_literal_operands(
                self.ast_arena,
                left,
                right,
                l_base,
                r_base,
                &self.ty_table,
            );
            let same_numeric = both_numeric
                && varn_binder::type_inference::numeric_operands_compatible(
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
                    same_numeric
                        || (l_base == Type::Str && r_base == Type::Str)
                        || (l_base == Type::Char && r_base == Type::Char)
                }
                BinaryOp::Eq | BinaryOp::NotEq => {
                    if both_numeric {
                        same_numeric
                    } else {
                        l_ty == Type::Null
                            || r_ty == Type::Null
                            || self.value_fits(rec, &l_ty, &r_ty, Some(right), Some(bind))
                            || self.value_fits(rec, &r_ty, &l_ty, Some(left), Some(bind))
                    }
                }
                BinaryOp::Instanceof | BinaryOp::In => true,
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
