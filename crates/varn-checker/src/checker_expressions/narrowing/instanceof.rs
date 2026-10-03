use crate::binder::BindResult;
use crate::checker::Checker;
use crate::symbol::SymbolId;
use crate::types::Type;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{ExprId, ExprKind};

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
        if op == BinaryOp::Instanceof {
            if let (ExprKind::Identifier { name }, ExprKind::Identifier { name: class_name }) =
                (&arena.expr(left).kind, &arena.expr(right).kind)
            {
                let (name, class_name) = (*name, *class_name);
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(name, &bind.scopes) {
                    let class_name_str = bind.interner.resolve(class_name).to_string();
                    if is_true_branch {
                        let named = Type::named(
                            class_name_str,
                            self.resolver,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        );
                        out.push((id, named));
                    } else if let Some(ty) = bind.arena.get(id).ty {
                        let class_name_atom = self.resolver.intern(&class_name_str);
                        let narrowed = ty.minus_named(
                            class_name_atom,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        );
                        if bind.interner.get("_") == Some(name) {
                            out.push((id, narrowed));
                        }
                    }
                }
            }
        }
    }
}
