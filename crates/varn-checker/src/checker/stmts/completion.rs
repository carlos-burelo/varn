use super::super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::{AstArena, AstId, ExprKind, StmtId, StmtKind};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::output::TypeEntry;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn require_return(&mut self, rec: &Recorder, body: StmtId, bind: &BindResult) {
        let Some(declared) = self.expected_return_type else {
            return;
        };
        if declared.is_dynamic() || declared == Type::Void {
            return;
        }
        if !completes(body, self.ast_arena, &rec.expr_table) {
            return;
        }
        let range = self.ast_arena.stmt(body).range;
        let message = if declared == Type::Never {
            "a function declared to return 'never' cannot reach its end: it must throw or loop"
                .to_owned()
        } else {
            format!(
                "not every path returns a value: the function is declared to return '{}'",
                declared.display(&self.ty_table, &bind.interner)
            )
        };
        self.emit(Diagnostic::error(ErrorCode::MissingReturn, message).with_range(range));
    }
}

fn completes(
    stmt: StmtId,
    arena: &AstArena,
    table: &rustc_hash::FxHashMap<AstId, TypeEntry>,
) -> bool {
    match &arena.stmt(stmt).kind {
        StmtKind::Return { .. } | StmtKind::Throw { .. } => false,
        StmtKind::Block { stmts } => stmts.iter().all(|s| completes(*s, arena, table)),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => match alternate {
            Some(alt) => completes(*consequent, arena, table) || completes(*alt, arena, table),
            None => true,
        },
        StmtKind::While { test, body } | StmtKind::DoWhile { body, test } => {
            let forever = matches!(
                arena.expr(*test).kind,
                ExprKind::BoolLiteral { value: true }
            );
            !forever || breaks(*body, arena)
        }
        StmtKind::For { test, body, .. } => test.is_some() || breaks(*body, arena),
        StmtKind::Switch { cases, .. } => {
            let has_default = cases.iter().any(|c| c.test.is_none());
            let escapes = cases
                .iter()
                .any(|c| c.body.iter().any(|s| breaks(*s, arena)));
            let falls_out = cases
                .last()
                .is_none_or(|c| c.body.iter().all(|s| completes(*s, arena, table)));
            !has_default || escapes || falls_out
        }
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            let ends = finally.is_none_or(|f| completes(f, arena, table));
            let reaches = completes(*block, arena, table)
                || catches.iter().any(|c| completes(c.body, arena, table));
            ends && reaches
        }
        StmtKind::Labeled { body, .. } => completes(*body, arena, table),
        StmtKind::Expr { expression } => !diverges(*expression, arena, table),
        StmtKind::Empty
        | StmtKind::Decl(_)
        | StmtKind::Error
        | StmtKind::ForIn { .. }
        | StmtKind::ForOf { .. }
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. }
        | StmtKind::Using { .. }
        | StmtKind::Debugger => true,
    }
}

fn diverges(
    expr: varn_core::ast::ExprId,
    arena: &AstArena,
    table: &rustc_hash::FxHashMap<AstId, TypeEntry>,
) -> bool {
    let ExprKind::Call { .. } = &arena.expr(expr).kind else {
        return false;
    };
    table
        .get(&expr.index())
        .is_some_and(|entry| entry.ty.is_never())
}

fn breaks(stmt: StmtId, arena: &AstArena) -> bool {
    match &arena.stmt(stmt).kind {
        StmtKind::Break { .. } => true,
        StmtKind::Block { stmts } => stmts.iter().any(|s| breaks(*s, arena)),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => breaks(*consequent, arena) || alternate.is_some_and(|alt| breaks(alt, arena)),
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            breaks(*block, arena)
                || catches.iter().any(|c| breaks(c.body, arena))
                || finally.is_some_and(|f| breaks(f, arena))
        }
        StmtKind::Labeled { body, .. } => breaks(*body, arena),
        StmtKind::Switch { cases, .. } => cases.iter().any(|c| {
            c.body
                .iter()
                .any(|s| matches!(arena.stmt(*s).kind, StmtKind::Break { label: Some(_) }))
        }),
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::ForOf { body, .. } => labeled_break(*body, arena),
        StmtKind::Empty
        | StmtKind::Expr { .. }
        | StmtKind::Decl(_)
        | StmtKind::Error
        | StmtKind::Return { .. }
        | StmtKind::Throw { .. }
        | StmtKind::Continue { .. }
        | StmtKind::Using { .. }
        | StmtKind::Debugger => false,
    }
}

fn labeled_break(stmt: StmtId, arena: &AstArena) -> bool {
    match &arena.stmt(stmt).kind {
        StmtKind::Break { label } => label.is_some(),
        StmtKind::Block { stmts } => stmts.iter().any(|s| labeled_break(*s, arena)),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            labeled_break(*consequent, arena)
                || alternate.is_some_and(|alt| labeled_break(alt, arena))
        }
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            labeled_break(*block, arena)
                || catches.iter().any(|c| labeled_break(c.body, arena))
                || finally.is_some_and(|f| labeled_break(f, arena))
        }
        StmtKind::Labeled { body, .. }
        | StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::ForOf { body, .. } => labeled_break(*body, arena),
        StmtKind::Switch { cases, .. } => cases
            .iter()
            .any(|c| c.body.iter().any(|s| labeled_break(*s, arena))),
        StmtKind::Empty
        | StmtKind::Expr { .. }
        | StmtKind::Decl(_)
        | StmtKind::Error
        | StmtKind::Return { .. }
        | StmtKind::Throw { .. }
        | StmtKind::Continue { .. }
        | StmtKind::Using { .. }
        | StmtKind::Debugger => false,
    }
}
