use super::super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::ExprId;
use varn_core::{Diagnostic, ErrorCode, SourceRange, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_return_stmt(
        &mut self,
        rec: &mut Recorder,
        argument: Option<ExprId>,
        range: SourceRange,
        bind: &BindResult,
    ) {
        if !self.in_function {
            self.emit(
                Diagnostic::error(
                    ErrorCode::ReturnOutsideFunction,
                    "a 'return' statement can only be used within a function body",
                )
                .with_range(range),
            );
        }

        let actual = if let Some(arg) = argument {
            let expected_ret = self.expected_return_type;
            if expected_ret.is_some_and(|ty| self.can_defer_decoder_return(ty, bind)) {
                let mut tail = arg;
                while let varn_core::ast::ExprKind::Paren { expression } =
                    &self.ast_arena.expr(tail).kind
                {
                    tail = *expression;
                }
                rec.decoder_returns.insert(tail.index());
            }
            self.with_expected(expected_ret, |c| c.check_expr(rec, arg, bind));
            self.infer_type(rec, arg, bind)
        } else {
            Type::Void
        };

        if let Some(expected) = self.expected_return_type {
            let expected_kind = self.ty_table.get(expected.0);
            let check_expected = if matches!(expected_kind, TypeKind::TypePredicate { .. }) {
                Type::Bool
            } else if self.declared_yield.is_some() {
                varn_sem::types::yielded_by(expected, &self.ty_table)
            } else {
                expected
            };
            let deferred = argument.is_some()
                && actual.is_dynamic()
                && self.can_defer_decoder_return(check_expected, bind);
            if !deferred && !self.value_fits(rec, &check_expected, &actual, argument, Some(bind)) {
                let expected_s = expected.display(&self.ty_table, &bind.interner);
                let actual_s = actual.display(&self.ty_table, &bind.interner);
                self.emit(
                    Diagnostic::error(ErrorCode::TypeMismatch, format!(
                        "type mismatch: function is declared to return '{expected_s}', but returns '{actual_s}'"
                    ))
                    .with_range(range),
                );
            }
        }
    }

    pub(super) fn check_break_stmt(&mut self, range: SourceRange) {
        if self.loop_depth == 0 && self.switch_depth == 0 {
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidBreakTarget,
                    "a 'break' statement can only be used within an enclosing iteration or switch statement",
                )
                .with_range(range),
            );
        }
    }

    pub(super) fn check_jump_label(
        &mut self,
        label: Option<varn_core::Atom>,
        code: ErrorCode,
        range: SourceRange,
        bind: &BindResult,
    ) {
        let Some(label) = label else {
            return;
        };
        if self.loop_labels.contains(&label) {
            return;
        }
        self.emit(
            Diagnostic::error(
                code,
                format!(
                    "no enclosing loop is labelled '{}'",
                    bind.interner.resolve(label)
                ),
            )
            .with_range(range),
        );
    }

    pub(super) fn check_continue_stmt(&mut self, range: SourceRange) {
        if self.loop_depth == 0 {
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidContinueTarget,
                    "a 'continue' statement can only be used within an enclosing iteration statement",
                )
                .with_range(range),
            );
        }
    }
}
