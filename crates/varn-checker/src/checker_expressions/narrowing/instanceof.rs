use crate::checker::Checker;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{ExprId, ExprKind};
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn narrow_instanceof(
        &mut self,
        left: ExprId,
        right: ExprId,
        op: BinaryOp,
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        let arena = self.ast_arena;
        if op == BinaryOp::Instanceof
            && let (ExprKind::Identifier { name }, ExprKind::Identifier { name: class_name }) =
                (&arena.expr(left).kind, &arena.expr(right).kind)
        {
            let (name, class_name) = (*name, *class_name);
            let scope = bind.scopes.get(self.current_scope);
            if let Some(id) = scope.resolve(name, &bind.scopes) {
                let class_name_str = bind.interner.resolve(class_name).to_string();
                let view =
                    varn_sem::bind::BindView::new(bind, self.resolver).at(self.current_scope);
                let table = std::sync::Arc::make_mut(&mut self.ty_table);
                let origin = varn_binder::nominal_origin(&class_name_str, Some(&view), table);
                if is_true_branch {
                    let named = Type::named_with_origin_atom(class_name, origin, table);
                    out.push((id, named));
                } else if let Some(ty) = self.narrowing_base(id, bind.arena.get(id).ty) {
                    let narrowed = ty.minus_nominal(
                        class_name,
                        origin,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    if bind
                        .interner
                        .get(varn_core::TokenKind::Placeholder.as_str())
                        == Some(name)
                    {
                        out.push((id, narrowed));
                    }
                }
            }
        }
    }
}
