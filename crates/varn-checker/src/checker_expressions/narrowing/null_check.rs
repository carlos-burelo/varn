use crate::checker::Checker;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{ExprId, ExprKind};
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn narrow_null_comparison(
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
        let (ident_name, is_null_check) = match (&arena.expr(left).kind, &arena.expr(right).kind) {
            (ExprKind::Identifier { name }, ExprKind::NullLiteral) => (Some(*name), true),
            (ExprKind::NullLiteral, ExprKind::Identifier { name }) => (Some(*name), true),
            _ => (None, false),
        };

        if is_null_check {
            if let Some(name) = ident_name {
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(name, &bind.scopes) {
                    if (is_neq && is_true_branch) || (is_eq && !is_true_branch) {
                        let original_ty = self
                            .symbol_types
                            .get(&id)
                            .cloned()
                            .or_else(|| bind.arena.get(id).ty);
                        if let Some(ty) = original_ty {
                            let narrowed = ty
                                .non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                            if !narrowed.is_dynamic() {
                                out.push((id, narrowed));
                            }
                        }
                    } else {
                        let name_str = bind.interner.resolve(name);
                        if name_str != "_" && name_str != "__variant__" {
                            out.push((id, Type::Null));
                        }
                    }
                }
            }
        }
    }
}
