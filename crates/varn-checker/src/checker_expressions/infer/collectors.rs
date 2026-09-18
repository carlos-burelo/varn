use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::{StmtId, StmtKind};

pub(crate) fn collect_checked_return_types(
    stmt: StmtId,
    checker: &mut Checker,
    bind: &crate::binder::BindResult,
) -> Vec<Type> {
    let mut out = Vec::new();
    collect_returns(stmt, checker, bind, &mut out);
    out
}

fn collect_returns(
    stmt: StmtId,
    checker: &mut Checker,
    bind: &crate::binder::BindResult,
    out: &mut Vec<Type>,
) {
    let arena = checker.ast_arena;
    match &arena.stmt(stmt).kind {
        StmtKind::Block { stmts, .. } => {
            for s in stmts.clone() {
                collect_returns(s, checker, bind, out);
            }
        }
        StmtKind::Return {
            argument: Some(e), ..
        } => {
            let ty = checker.infer_type(*e, bind);
            if !ty.is_dynamic() {
                out.push(ty);
            }
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            let (consequent, alternate) = (*consequent, *alternate);
            collect_returns(consequent, checker, bind, out);
            if let Some(alt) = alternate {
                collect_returns(alt, checker, bind, out);
            }
        }
        StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } => {
            collect_returns(*body, checker, bind, out);
        }
        StmtKind::For { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::ForOf { body, .. } => {
            collect_returns(*body, checker, bind, out);
        }
        StmtKind::Try {
            block,
            catches,
            finally,
            ..
        } => {
            let (block, catches, finally) = (*block, catches.clone(), *finally);
            collect_returns(block, checker, bind, out);
            for c in &catches {
                collect_returns(c.body, checker, bind, out);
            }
            if let Some(f) = finally {
                collect_returns(f, checker, bind, out);
            }
        }
        StmtKind::Labeled { body, .. } => collect_returns(*body, checker, bind, out),
        StmtKind::Switch { cases, .. } => {
            for case in cases.clone() {
                for s in &case.body {
                    collect_returns(*s, checker, bind, out);
                }
            }
        }

        _ => {}
    }
}
