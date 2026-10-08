use super::super::Checker;
use varn_core::ast::{StmtId, StmtKind};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(crate) fn check_stmt(&mut self, stmt: StmtId, bind: &BindResult) {
        let arena = self.ast_arena;
        let range = arena.stmt(stmt).range;
        match &arena.stmt(stmt).kind {
            StmtKind::Decl(decl) => {
                let decl = decl.clone();
                self.check_decl(&decl, bind);
            }

            StmtKind::Block { stmts } => {
                let stmts = stmts.clone();
                self.with_next_child_scope_span(
                    bind,
                    range.start.offset,
                    range.end.offset,
                    |checker| checker.check_stmts(&stmts, bind),
                );
            }

            StmtKind::Expr { expression } => {
                let expression = *expression;
                self.check_expr(expression, bind);
            }

            StmtKind::Return { argument } => {
                self.check_return_stmt(*argument, range, bind);
            }

            StmtKind::Break { .. } => {
                self.check_break_stmt(range);
            }

            StmtKind::Continue { .. } => {
                self.check_continue_stmt(range);
            }

            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.check_if_stmt(*test, *consequent, *alternate, bind);
            }

            StmtKind::While { test, body } | StmtKind::DoWhile { test, body } => {
                self.check_while_stmt(*test, *body, bind);
            }

            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                self.check_for_stmt(init.clone(), *test, *update, *body, range, bind);
            }

            StmtKind::ForOf {
                left, right, body, ..
            } => {
                self.check_for_of_stmt(left.clone(), *right, *body, range, bind);
            }

            StmtKind::ForIn {
                left, right, body, ..
            } => {
                self.check_for_in_stmt(left.clone(), *right, *body, range, bind);
            }

            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                self.check_switch_stmt(*discriminant, cases.clone(), bind);
            }

            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                self.check_try_stmt(*block, catches.clone(), *finally, bind);
            }

            StmtKind::Throw { argument } => {
                self.check_throw_stmt(*argument, bind);
            }

            StmtKind::Labeled { body, .. } => {
                let body = *body;
                self.check_stmt(body, bind);
            }

            StmtKind::Using {
                declarations,
                is_await,
                ..
            } => {
                self.check_using_stmt(declarations.clone(), *is_await, bind);
            }

            StmtKind::Empty | StmtKind::Error | StmtKind::Debugger => {}
        }
    }
}
