use crate::checker::Checker;
use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{ExprId, ExprKind};

impl<'r> Checker<'r> {
    pub(crate) fn can_extract_narrowings(&self, expr: ExprId) -> bool {
        matches!(
            &self.ast_arena.expr(expr).kind,
            ExprKind::Binary {
                op: BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Instanceof,
                ..
            } | ExprKind::Logical { .. }
                | ExprKind::Is { .. }
                | ExprKind::Call { .. }
                | ExprKind::Identifier { .. }
                | ExprKind::Unary {
                    op: UnaryOp::Not,
                    ..
                }
        )
    }
}
