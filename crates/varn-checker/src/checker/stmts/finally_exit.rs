use crate::checker::Checker;
use varn_core::ast::{StmtId, StmtKind};
use varn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn reject_return_in_finally(&mut self, finally: StmtId) {
        let arena = self.ast_arena;
        match &arena.stmt(finally).kind {
            StmtKind::Return { .. } => self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidFinallyExit,
                    "`return` inside `finally` would replace the result or error of the try block",
                )
                .with_range(arena.stmt(finally).range),
            ),
            StmtKind::Block { stmts } => stmts.iter().for_each(|&s| self.reject_return_in_finally(s)),
            StmtKind::If {
                consequent,
                alternate,
                ..
            } => {
                self.reject_return_in_finally(*consequent);
                if let Some(a) = alternate {
                    self.reject_return_in_finally(*a);
                }
            }
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::ForOf { body, .. }
            | StmtKind::Labeled { body, .. } => self.reject_return_in_finally(*body),
            StmtKind::Switch { cases, .. } => {
                for case in cases {
                    case.body.iter().for_each(|&s| self.reject_return_in_finally(s));
                }
            }
            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                self.reject_return_in_finally(*block);
                catches.iter().for_each(|c| self.reject_return_in_finally(c.body));
                if let Some(f) = finally {
                    self.reject_return_in_finally(*f);
                }
            }
            StmtKind::Expr { .. }
            | StmtKind::Decl(_)
            | StmtKind::Throw { .. }
            | StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Using { .. }
            | StmtKind::Empty
            | StmtKind::Debugger
            | StmtKind::Error => {}
        }
    }
}
