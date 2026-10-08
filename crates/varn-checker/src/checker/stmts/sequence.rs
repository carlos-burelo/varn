use super::super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::{StmtId, StmtKind};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn check_stmts(&mut self, rec: &mut Recorder, stmts: &[StmtId], bind: &BindResult) {
        self.check_stmts_with_guards(rec, stmts, bind);
    }

    fn check_stmts_with_guards(&mut self, rec: &mut Recorder, stmts: &[StmtId], bind: &BindResult) {
        let arena = self.ast_arena;
        let mut i = 0;
        while i < stmts.len() {
            let stmt_id = stmts[i];
            let stmt = arena.stmt(stmt_id);

            if matches!(&stmt.kind, StmtKind::Throw { .. } | StmtKind::Return { .. }) {
                self.check_stmt(rec, stmt_id, bind);

                for &later_id in &stmts[i + 1..] {
                    let later = arena.stmt(later_id);
                    self.emit(
                        Diagnostic::warning(ErrorCode::UnreachableCode, "unreachable code")
                            .with_range(later.range),
                    );
                }
                return;
            }

            if let Some(guard_narrowings) = self.extract_guard_narrowings(rec, stmt_id, bind) {
                self.check_stmt(rec, stmt_id, bind);

                self.push_narrowings(&guard_narrowings);
                self.check_stmts_with_guards(rec, &stmts[i + 1..], bind);
                self.pop_narrowings(&guard_narrowings);
                return;
            }

            self.check_stmt(rec, stmt_id, bind);
            i += 1;
        }
    }

    fn extract_guard_narrowings(
        &mut self,
        rec: &mut Recorder,
        stmt: StmtId,
        bind: &BindResult,
    ) -> Option<Vec<(SymbolId, Type)>> {
        let (test, consequent, alternate) = match &self.ast_arena.stmt(stmt).kind {
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => (*test, *consequent, *alternate),
            StmtKind::Block { .. }
            | StmtKind::Empty
            | StmtKind::Expr { .. }
            | StmtKind::Decl(_)
            | StmtKind::Error
            | StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::ForIn { .. }
            | StmtKind::ForOf { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Return { .. }
            | StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Throw { .. }
            | StmtKind::Try { .. }
            | StmtKind::Using { .. }
            | StmtKind::Labeled { .. }
            | StmtKind::Debugger => return None,
        };
        if alternate.is_some() {
            return None;
        }
        if super::super::completion::can_complete_normally(consequent, self.ast_arena) {
            return None;
        }
        if !self.can_extract_narrowings(test) {
            return None;
        }
        let narrowings = self.extract_narrowings(rec, test, bind, false);
        if narrowings.is_empty() {
            None
        } else {
            Some(narrowings)
        }
    }

    pub(crate) fn with_narrowings(
        &mut self,
        narrowings: &[(varn_sem::symbol::SymbolId, Type)],
        f: impl FnOnce(&mut Self),
    ) {
        if narrowings.is_empty() {
            f(self);
            return;
        }

        self.push_narrowings(narrowings);
        f(self);
        self.pop_narrowings(narrowings);
    }

    fn push_narrowings(&mut self, narrowings: &[(varn_sem::symbol::SymbolId, Type)]) {
        for (id, ty) in narrowings {
            self.narrowed_types.entry(*id).or_default().push(*ty);
        }
        self.mark_infer_env_dirty();
    }

    fn pop_narrowings(&mut self, narrowings: &[(varn_sem::symbol::SymbolId, Type)]) {
        for (id, _) in narrowings {
            if let Some(stack) = self.narrowed_types.get_mut(id) {
                stack.pop();
            }
        }
        self.mark_infer_env_dirty();
    }
}
