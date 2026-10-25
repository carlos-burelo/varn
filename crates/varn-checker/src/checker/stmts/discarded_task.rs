use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl<'r> Checker<'r> {
    pub(super) fn reject_discarded_task(
        &mut self,
        rec: &mut Recorder,
        expression: ExprId,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let ExprKind::Call { callee, .. } = &arena.expr(expression).kind else {
            return;
        };
        let (callee_symbol, callee_name) = match &arena.expr(*callee).kind {
            ExprKind::Identifier { name } => (
                bind.scopes
                    .get(self.current_scope)
                    .resolve(*name, &bind.scopes),
                bind.interner.resolve(*name),
            ),
            ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } => match &arena.expr(*property).kind {
                ExprKind::Identifier { name } => {
                    let member = bind.interner.resolve(*name);
                    let owner = self.infer_type(rec, *object, bind);
                    let owner = owner.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                    let found = self
                        .find_member_info(&owner, member, bind)
                        .and_then(|(_, symbol)| symbol);
                    (found, member)
                }
                _ => return,
            },
            _ => return,
        };
        let declared_here = |symbol: usize| {
            symbol < bind.arena.len()
                && bind.interner.get(callee_name) == Some(bind.arena.get(symbol).name)
        };
        if callee_symbol.is_some_and(|symbol| declared_here(symbol) && bind.arena.get(symbol).is_async) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::UnusedTask,
                    "this async call creates a Task that is dropped: a Task only runs when awaited, so `await` it or `spawn` it",
                )
                .with_range(arena.expr(expression).range),
            );
        }
    }
}
