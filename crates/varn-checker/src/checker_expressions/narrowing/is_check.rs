use crate::binder::BindResult;
use crate::checker::Checker;
use crate::symbol::SymbolId;
use crate::types::Type;
use varn_core::ast::{ExprId, ExprKind, TypeNode};

impl<'r> Checker<'r> {
    pub(crate) fn narrow_is(
        &mut self,
        expression: ExprId,
        type_ann: TypeNode,
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        let arena = self.ast_arena;
        if let ExprKind::Identifier { name: arg_name } = &arena.expr(expression).kind {
            let arg_name = *arg_name;
            let scope = bind.scopes.get(self.current_scope);
            if let Some(id) = scope.resolve(arg_name, &bind.scopes) {
                if is_true_branch {
                    let view = crate::binder::BindView::new(bind, self.resolver);
                    let narrowed_ty = crate::binder::resolve_type_node(
                        &type_ann,
                        Some(&view),
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    out.push((id, narrowed_ty));
                } else {
                    if let Some(original_ty) = bind.arena.get(id).ty {
                        let view = crate::binder::BindView::new(bind, self.resolver);
                        let target_ty = crate::binder::resolve_type_node(
                            &type_ann,
                            Some(&view),
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        );
                        let narrowed = original_ty.minus(
                            &target_ty,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        );
                        if narrowed != original_ty {
                            out.push((id, narrowed));
                        }
                    }
                }
            }
        }
    }
}
