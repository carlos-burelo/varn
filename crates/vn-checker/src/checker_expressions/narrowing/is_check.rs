use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use vn_core::ast::{ExprId, ExprKind, TypeNode};
use vn_sem::bind::BindResult;
use vn_sem::symbol::SymbolId;
use vn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn narrow_is(
        &mut self,
        rec: &Recorder,
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
                    let view =
                        vn_sem::bind::BindView::new(bind, self.resolver).at(self.current_scope);
                    let narrowed_ty = vn_sem::resolve::resolve_type_node(
                        &type_ann,
                        Some(&view),
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    out.push((id, narrowed_ty));
                } else {
                    if let Some(original_ty) = self.narrowing_base(rec, id, bind) {
                        let view =
                            vn_sem::bind::BindView::new(bind, self.resolver).at(self.current_scope);
                        let target_ty = vn_sem::resolve::resolve_type_node(
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
