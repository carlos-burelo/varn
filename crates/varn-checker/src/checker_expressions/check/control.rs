use super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::ExprId;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn check_try(
        &mut self,
        rec: &mut Recorder,
        expression: ExprId,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        self.check_expr(rec, expression, bind);
        let expr_ty = self.infer_type(rec, expression, bind);

        let resolve = |a| bind.interner.try_resolve(a);
        let operand = expr_ty.core_sum(&self.ty_table, resolve);
        let is_nullable = expr_ty.is_nullable(&self.ty_table);

        if !expr_ty.is_dynamic() && operand.is_none() && !is_nullable {
            let expr_ty_s = expr_ty.display(&self.ty_table, &bind.interner);
            let shadowed = matches!(
                self.ty_table.get(expr_ty.0),
                varn_core::TypeKind::Generic(name, _, _) if matches!(resolve(name), Some("Result" | "Option"))
            );
            let note = if shadowed {
                " (a user declaration shadows the core type here)"
            } else {
                ""
            };
            self.emit(
                Diagnostic::error(
                    ErrorCode::TypeMismatch,
                    format!("operator 'try' can only be applied to 'Result', 'Option', or nullable types, found '{expr_ty_s}'{note}"),
                )
                .with_range(range),
            );
            return;
        }

        let Some(expected_ret) = self.expected_return_type else {
            self.emit(
                Diagnostic::error(
                    ErrorCode::TypeMismatch,
                    "operator 'try' cannot be used outside of a function".to_string(),
                )
                .with_range(range),
            );
            return;
        };

        if expected_ret.is_dynamic() {
            return;
        }

        let enclosing = expected_ret.core_sum(&self.ty_table, resolve);
        match (operand, enclosing) {
            (Some((sum, args)), Some((ret_sum, ret_args))) if sum == ret_sum => {
                if sum == varn_core::CoreSum::Result {
                    if let (Some(err_e), Some(err_r)) = (args.get(1), ret_args.get(1)) {
                        if !self.types_compatible_cached(err_r, err_e, Some(bind)) {
                            let err_e_s = err_e.display(&self.ty_table, &bind.interner);
                            let err_r_s = err_r.display(&self.ty_table, &bind.interner);
                            self.emit(
                                Diagnostic::error(
                                    ErrorCode::TypeMismatch,
                                    format!("cannot propagate error type '{err_e_s}' into return type '{err_r_s}'"),
                                )
                                .with_range(range),
                            );
                        }
                    }
                }
            }
            (Some((sum, _)), _) => {
                let expected_s = expected_ret.display(&self.ty_table, &bind.interner);
                let name = sum.name();
                self.emit(
                    Diagnostic::error(
                        ErrorCode::TypeMismatch,
                        format!("operator 'try' on '{name}' requires enclosing function to return '{name}', found '{expected_s}'"),
                    )
                    .with_range(range),
                );
            }
            (None, _) if !expected_ret.is_nullable(&self.ty_table) => {
                let expected_s = expected_ret.display(&self.ty_table, &bind.interner);
                self.emit(
                    Diagnostic::error(
                        ErrorCode::TypeMismatch,
                        format!("operator 'try' on nullable type requires enclosing function to return a nullable type, found '{expected_s}'"),
                    )
                    .with_range(range),
                );
            }
            (None, _) => {}
        }
    }
}
