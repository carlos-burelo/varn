use super::super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::{CatchClause, ExprId, StmtId};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn check_try_stmt(
        &mut self,
        rec: &mut Recorder,
        block: StmtId,
        catches: Vec<CatchClause>,
        finally: Option<StmtId>,
        bind: &BindResult,
    ) {
        self.check_stmt(rec, block, bind);
        for clause in &catches {
            self.with_next_child_scope(
                rec,
                bind,
                self.ast_arena.stmt(clause.body).range.start.offset,
                |checker, rec| {
                    if let Some(param) = &clause.param {
                        let catch_ty = if let Some(ann) = &clause.type_ann {
                            checker.resolve_type_node_cached(ann, bind)
                        } else {
                            Type::named(
                                "Error",
                                &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                            )
                        };
                        checker.check_pattern(rec, param, &catch_ty, bind);
                    }
                    checker.check_stmt(rec, clause.body, bind);
                },
            );
        }
        if let Some(fin) = finally {
            self.check_stmt(rec, fin, bind);
        }
    }

    pub(super) fn check_throw_stmt(
        &mut self,
        rec: &mut Recorder,
        argument: ExprId,
        bind: &BindResult,
    ) {
        if self.pure_scope.is_some() {
            self.forbid_pure(
                "throw (pure functions cannot raise)",
                self.ast_arena.expr(argument).range,
            );
        }
        self.check_expr(rec, argument, bind);
        let thrown = self.infer_type(rec, argument, bind);
        if !self.is_throwable(&thrown, bind) {
            let thrown_s = thrown.display(&self.ty_table, &bind.interner);
            self.emit(
                Diagnostic::error(
                    ErrorCode::InvalidThrowOperand,
                    format!(
                        "cannot throw a value of type `{thrown_s}`: thrown values must be `Error` or a subclass"
                    ),
                )
                .with_range(self.ast_arena.expr(argument).range),
            );
        }
    }
}
