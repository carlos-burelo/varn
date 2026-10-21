use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{ExprId, ExprKind};
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn narrow_typeof(
        &mut self,
        rec: &Recorder,
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

        let Some((typeof_op, value)) = typeof_check else {
            return;
        };
        if !is_eq && !is_neq {
            return;
        }
        let ExprKind::Identifier { name } = &arena.expr(typeof_op).kind else {
            return;
        };
        let Some(id) = bind
            .scopes
            .get(self.current_scope)
            .resolve(*name, &bind.scopes)
        else {
            return;
        };
        let view = varn_sem::bind::BindView::new(bind, self.resolver);
        let named = varn_binder::resolve_primitive(
            &value,
            Some(&view),
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        if is_eq == is_true_branch {
            if let Some(current) = self.narrowing_base(rec, id, bind) {
                let kind = self.ty_table.get(current.0);
                let already = (matches!(kind, varn_core::TypeKind::Tuple(_))
                    && value == varn_core::RuntimeKind::Tuple.name())
                    || (matches!(kind, varn_core::TypeKind::Array(_))
                        && value == varn_core::RuntimeKind::Array.name());
                if already {
                    return;
                }
            }
            out.push((id, named));
            return;
        }
        let Some(current) = self.narrowing_base(rec, id, bind) else {
            return;
        };
        let varn_core::TypeKind::Union(members) = self.ty_table.get(current.0) else {
            return;
        };
        let members: Vec<Type> = self
            .ty_table
            .get_list(members)
            .iter()
            .map(|m| Type::resolved(*m))
            .collect();
        let rest: Vec<Type> = members
            .iter()
            .copied()
            .filter(|m| !self.types_compatible_cached(&named, m, Some(bind)))
            .collect();
        if rest.is_empty() || rest.len() == members.len() {
            return;
        }
        let narrowed = Type::union(rest, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
        out.push((id, narrowed));
    }
}
