use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::ExprId;
use varn_core::{BuiltinType, Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn reject_discarded_task(
        &mut self,
        rec: &mut Recorder,
        expression: ExprId,
        bind: &BindResult,
    ) {
        if !matches!(
            self.ast_arena.expr(expression).kind,
            varn_core::ast::ExprKind::Call { .. }
        ) {
            return;
        }
        let ty = self.infer_type(rec, expression, bind);
        if ty.builtin_of(&self.ty_table) != Some(BuiltinType::Task) {
            return;
        }
        self.emit(
            Diagnostic::error(
                ErrorCode::UnusedTask,
                "this Task is created and dropped: a Task only runs when awaited, so `await` it or `spawn` it",
            )
            .with_range(self.ast_arena.expr(expression).range),
        );
    }
}
