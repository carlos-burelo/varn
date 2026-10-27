use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::{ArrowBody, StmtId, StmtKind};
use varn_sem::types::Type;

pub(crate) fn arrow_body_return_type(
    body: ArrowBody,
    checker: &mut Checker,
    rec: &mut Recorder,
    bind: &varn_sem::bind::BindResult,
) -> Type {
    match body {
        ArrowBody::Expr(e) => {
            let saved_pipeline = checker.in_pipeline_rhs;
            let saved_pipe_ty = checker.pipeline_value_type;
            checker.in_pipeline_rhs = false;
            checker.pipeline_value_type = None;
            let t = checker.infer_type(rec, e, bind);
            checker.in_pipeline_rhs = saved_pipeline;
            checker.pipeline_value_type = saved_pipe_ty;
            t
        }
        ArrowBody::Block(block) => {
            let mut returns = Returns::default();
            collect_returns(block, checker, rec, bind, &mut returns);
            if returns.dynamic {
                return Type::Dynamic;
            }
            let mut return_tys = returns.typed;
            if !return_tys.is_empty()
                && crate::checker::completion::can_complete_normally(block, checker.ast_arena)
            {
                return_tys.push(Type::Void);
            }
            match return_tys.len() {
                0 if !crate::checker::completion::can_complete_normally(
                    block,
                    checker.ast_arena,
                ) =>
                {
                    Type::Never
                }
                0 => Type::Void,
                1 => return_tys.pop().expect("one return type"),
                _ => Type::union(
                    return_tys,
                    &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                ),
            }
        }
    }
}

#[derive(Default)]
struct Returns {
    typed: Vec<Type>,
    dynamic: bool,
}

fn collect_returns(
    stmt: StmtId,
    checker: &mut Checker,
    rec: &mut Recorder,
    bind: &varn_sem::bind::BindResult,
    out: &mut Returns,
) {
    let arena = checker.ast_arena;
    match &arena.stmt(stmt).kind {
        StmtKind::Block { stmts, .. } => {
            for s in stmts.clone() {
                collect_returns(s, checker, rec, bind, out);
            }
        }
        StmtKind::Return {
            argument: Some(e), ..
        } => {
            let ty = match rec.expr_table.get(&e.index()) {
                Some(entry) => entry.ty,
                None => checker.infer_type(rec, *e, bind),
            };
            if ty.is_dynamic() {
                out.dynamic = true;
            } else {
                out.typed.push(ty);
            }
        }
        StmtKind::Return { argument: None } => out.typed.push(Type::Void),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            let (consequent, alternate) = (*consequent, *alternate);
            collect_returns(consequent, checker, rec, bind, out);
            if let Some(alt) = alternate {
                collect_returns(alt, checker, rec, bind, out);
            }
        }
        StmtKind::While { body, .. } | StmtKind::DoWhile { body, .. } => {
            collect_returns(*body, checker, rec, bind, out);
        }
        StmtKind::For { body, .. }
        | StmtKind::ForIn { body, .. }
        | StmtKind::ForOf { body, .. } => {
            collect_returns(*body, checker, rec, bind, out);
        }
        StmtKind::Try {
            block,
            catches,
            finally,
            ..
        } => {
            let (block, catches, finally) = (*block, catches.clone(), *finally);
            collect_returns(block, checker, rec, bind, out);
            for c in &catches {
                collect_returns(c.body, checker, rec, bind, out);
            }
            if let Some(f) = finally {
                collect_returns(f, checker, rec, bind, out);
            }
        }
        StmtKind::Labeled { body, .. } => collect_returns(*body, checker, rec, bind, out),
        StmtKind::Switch { cases, .. } => {
            for case in cases.clone() {
                for s in &case.body {
                    collect_returns(*s, checker, rec, bind, out);
                }
            }
        }

        StmtKind::Empty
        | StmtKind::Expr { .. }
        | StmtKind::Decl(_)
        | StmtKind::Error
        | StmtKind::Break { .. }
        | StmtKind::Continue { .. }
        | StmtKind::Throw { .. }
        | StmtKind::Using { .. }
        | StmtKind::Debugger => {}
    }
}
