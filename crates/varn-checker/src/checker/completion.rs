//! Whether a statement can complete normally, i.e. let control fall through
//! to the next statement. `false` only when every path leaves by `return`,
//! `throw`, `break` or `continue`; loops, `switch` and labels are assumed to
//! complete, so an answer of `false` is always sound.

use varn_core::ast::{AstArena, StmtId, StmtKind};

pub(crate) fn can_complete_normally(stmt: StmtId, arena: &AstArena) -> bool {
    match &arena.stmt(stmt).kind {
        StmtKind::Return { .. }
        | StmtKind::Throw { .. }
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. } => false,
        StmtKind::Block { stmts } => stmts.iter().all(|&s| can_complete_normally(s, arena)),
        StmtKind::If {
            consequent,
            alternate: Some(alternate),
            ..
        } => can_complete_normally(*consequent, arena) || can_complete_normally(*alternate, arena),
        StmtKind::Try {
            block,
            catches,
            finally,
        } => {
            let body = can_complete_normally(*block, arena)
                || catches.iter().any(|c| can_complete_normally(c.body, arena));
            body && finally.is_none_or(|f| can_complete_normally(f, arena))
        }
        StmtKind::If {
            alternate: None, ..
        }
        | StmtKind::Empty
        | StmtKind::Debugger
        | StmtKind::Expr { .. }
        | StmtKind::Decl(_)
        | StmtKind::Error
        | StmtKind::While { .. }
        | StmtKind::DoWhile { .. }
        | StmtKind::For { .. }
        | StmtKind::ForIn { .. }
        | StmtKind::ForOf { .. }
        | StmtKind::Switch { .. }
        | StmtKind::Using { .. }
        | StmtKind::Labeled { .. } => true,
    }
}
