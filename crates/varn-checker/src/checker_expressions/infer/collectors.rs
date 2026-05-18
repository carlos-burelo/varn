use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::{Stmt, StmtKind};

pub(crate) fn collect_checked_return_types(
    stmt: &Stmt,
    checker: &mut Checker,
    bind: &crate::binder::BindResult,
) -> Vec<Type> {
    let mut out = Vec::new();
    collect_returns(stmt, checker, bind, &mut out);
    out
}

fn collect_returns(
    stmt: &Stmt,
    checker: &mut Checker,
    bind: &crate::binder::BindResult,
    out: &mut Vec<Type>,
) {
    match &stmt.kind {
        StmtKind::Block { stmts, .. } => {
            for s in stmts {
                collect_returns(s, checker, bind, out);
            }
        }
        StmtKind::Return {
            argument: Some(e), ..
        } => {
            let ty = checker.infer_type(e, bind);
            if !ty.is_dynamic() {
                out.push(ty);
            }
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            collect_returns(consequent, checker, bind, out);
            if let Some(alt) = alternate {
                collect_returns(alt, checker, bind, out);
            }
        }
        StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } => {
            collect_returns(body, checker, bind, out);
        }
        StmtKind::For { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::ForOf { body, .. } => {
            collect_returns(body, checker, bind, out);
        }
        StmtKind::Try {
            block,
            catch,
            finally,
            ..
        } => {
            collect_returns(block, checker, bind, out);
            if let Some(c) = catch {
                collect_returns(c.body.as_ref(), checker, bind, out);
            }
            if let Some(f) = finally {
                collect_returns(f, checker, bind, out);
            }
        }
        StmtKind::Labeled { body, .. } => collect_returns(body, checker, bind, out),
        StmtKind::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    collect_returns(s, checker, bind, out);
                }
            }
        }

        _ => {}
    }
}
