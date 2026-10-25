use super::super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::{StmtId, StmtKind};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(crate) fn check_stmt(&mut self, rec: &mut Recorder, stmt: StmtId, bind: &BindResult) {
        let arena = self.ast_arena;
        let range = arena.stmt(stmt).range;
        match &arena.stmt(stmt).kind {
            StmtKind::Decl(decl) => {
                let decl = decl.clone();
                self.check_decl(rec, &decl, bind);
            }

            StmtKind::Block { stmts } => {
                let stmts = stmts.clone();
                self.with_next_child_scope_span(
                    rec,
                    bind,
                    range.start.offset,
                    range.end.offset,
                    |checker, rec| checker.check_stmts(rec, &stmts, bind),
                );
            }

            StmtKind::Expr { expression } => {
                let expression = *expression;
                self.check_expr(rec, expression, bind);
                self.reject_discarded_task(rec, expression, bind);
            }

            StmtKind::Return { argument } => {
                self.check_return_stmt(rec, *argument, range, bind);
            }

            StmtKind::Break { label } => {
                self.check_break_stmt(range);
                self.check_jump_label(
                    *label,
                    varn_core::ErrorCode::InvalidBreakTarget,
                    range,
                    bind,
                );
            }

            StmtKind::Continue { label } => {
                self.check_continue_stmt(range);
                self.check_jump_label(
                    *label,
                    varn_core::ErrorCode::InvalidContinueTarget,
                    range,
                    bind,
                );
            }

            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.check_if_stmt(rec, *test, *consequent, *alternate, bind);
            }

            StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
                self.forget_loop_assignments(rec, stmt, bind);
                self.check_while_stmt(rec, *test, *body, bind);
            }

            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                self.forget_loop_assignments(rec, stmt, bind);
                self.check_for_stmt(rec, init.clone(), *test, *update, *body, range, bind);
            }

            StmtKind::ForOf {
                left,
                right,
                body,
                is_await,
                ..
            } => {
                self.forget_loop_assignments(rec, stmt, bind);
                self.check_for_of_stmt(rec, left.clone(), *right, *body, *is_await, range, bind);
            }

            StmtKind::ForIn {
                left, right, body, ..
            } => {
                self.forget_loop_assignments(rec, stmt, bind);
                self.check_for_in_stmt(rec, left.clone(), *right, *body, range, bind);
            }

            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                self.check_switch_stmt(rec, *discriminant, cases.clone(), bind);
            }

            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                self.check_try_stmt(rec, *block, catches.clone(), *finally, bind);
            }

            StmtKind::Throw { argument } => {
                self.check_throw_stmt(rec, *argument, bind);
            }

            StmtKind::Labeled { label, body } => {
                let (label, body) = (*label, *body);
                let on_loop = matches!(
                    self.ast_arena.stmt(body).kind,
                    StmtKind::While { .. }
                        | StmtKind::DoWhile { .. }
                        | StmtKind::For { .. }
                        | StmtKind::ForIn { .. }
                        | StmtKind::ForOf { .. }
                );
                if !on_loop {
                    self.emit(
                        varn_core::Diagnostic::error(
                            varn_core::ErrorCode::InvalidBreakTarget,
                            "a label names a loop: put it on a 'for', 'while' or 'do'",
                        )
                        .with_range(range),
                    );
                }
                self.loop_labels.push(label);
                self.check_stmt(rec, body, bind);
                self.loop_labels.pop();
            }

            StmtKind::Using {
                declarations,
                is_await,
                ..
            } => {
                self.check_using_stmt(rec, declarations.clone(), *is_await, bind);
            }

            StmtKind::Empty | StmtKind::Error | StmtKind::Debugger => {}
        }
    }
}
