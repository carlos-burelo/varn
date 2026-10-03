use super::super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use varn_core::ast::{CatchClause, ExprId, StmtId};
use varn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn check_try_stmt(
        &mut self,
        block: StmtId,
        catches: Vec<CatchClause>,
        finally: Option<StmtId>,
        bind: &BindResult,
    ) {
        self.check_stmt(block, bind);
        for clause in &catches {
            self.with_next_child_scope(
                bind,
                self.ast_arena.stmt(clause.body).range.start.offset,
                |checker| {
                    if let Some(param) = &clause.param {
                        let catch_ty = if let Some(ann) = &clause.type_ann {
                            checker.resolve_type_node_cached(ann, bind)
                        } else {
                            Type::named(
                                "Error",
                                checker.resolver,
                                &mut *std::sync::Arc::make_mut(&mut checker.ty_table),
                            )
                        };
                        checker.check_pattern(param, &catch_ty, bind);
                    }
                    checker.check_stmt(clause.body, bind);
                },
            );
        }
        if let Some(fin) = finally {
            self.check_stmt(fin, bind);
        }
    }

    pub(super) fn check_throw_stmt(&mut self, argument: ExprId, bind: &BindResult) {
        self.check_expr(argument, bind);
        let thrown = self.infer_type(argument, bind);
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
