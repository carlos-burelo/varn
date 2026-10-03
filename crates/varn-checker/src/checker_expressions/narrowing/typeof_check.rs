use crate::binder::BindResult;
use crate::checker::Checker;
use crate::symbol::SymbolId;
use crate::types::Type;
use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{ExprId, ExprKind};

impl<'r> Checker<'r> {
    pub(crate) fn narrow_typeof(
        &mut self,
        left: ExprId,
        right: ExprId,
        op: BinaryOp,
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        let arena = self.ast_arena;
        let is_eq = op == BinaryOp::Eq;
        let is_neq = op == BinaryOp::NotEq;
        let typeof_check = match (&arena.expr(left).kind, &arena.expr(right).kind) {
            (
                ExprKind::Unary {
                    op: UnaryOp::Typeof,
                    operand: typeof_op,
                    ..
                },
                ExprKind::StrLiteral { value },
            ) => Some((*typeof_op, value.clone())),
            (
                ExprKind::StrLiteral { value },
                ExprKind::Unary {
                    op: UnaryOp::Typeof,
                    operand: typeof_op,
                    ..
                },
            ) => Some((*typeof_op, value.clone())),
            _ => None,
        };

        if let Some((typeof_op, value)) = typeof_check {
            if (is_eq && is_true_branch) || (is_neq && !is_true_branch) {
                if let ExprKind::Identifier { name } = &arena.expr(typeof_op).kind {
                    let scope = bind.scopes.get(self.current_scope);
                    if let Some(id) = scope.resolve(*name, &bind.scopes) {
                        let view = crate::binder::BindView::new(bind, self.resolver);
                        let narrowed_ty = crate::binder::resolve_primitive(
                            &value,
                            Some(&view),
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        );
                        out.push((id, narrowed_ty));
                    }
                }
            }
        }
    }
}
