use super::Checker;
use crate::binder::BindResult;
use varn_core::ast::ExprId;
use varn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn check_assign(
        &mut self,
        target: ExprId,
        value: ExprId,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let prev = self.is_assignment_target;
        self.is_assignment_target = true;
        self.check_expr(target, bind);
        self.is_assignment_target = prev;

        let target_ty =
            if let varn_core::ast::ExprKind::Identifier { name } = &arena.expr(target).kind {
                let name = *name;
                let scope = bind.scopes.get(self.current_scope);
                scope
                    .resolve(name, &bind.scopes)
                    .and_then(|id| {
                        self.symbol_types
                            .get(&id)
                            .cloned()
                            .or_else(|| bind.arena.get(id).ty)
                    })
                    .unwrap_or_else(|| self.infer_type(target, bind))
            } else {
                self.infer_type(target, bind)
            };

        let target_expected = if target_ty.is_dynamic() {
            None
        } else {
            Some(target_ty)
        };
        self.with_expected(target_expected, |c| c.check_expr(value, bind));

        self.check_extension_assignment(target, bind);

        if !matches!(
            &arena.expr(target).kind,
            varn_core::ast::ExprKind::Identifier { .. } | varn_core::ast::ExprKind::Member { .. }
        ) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    "invalid left-hand side in assignment",
                )
                .with_range(arena.expr(target).range),
            );
        }

        if let varn_core::ast::ExprKind::Identifier { name } = &arena.expr(target).kind {
            let name = *name;
            let scope = bind.scopes.get(self.current_scope);
            if let Some(id) = scope.resolve(name, &bind.scopes) {
                let sym = bind.arena.get(id);
                if sym.kind == crate::symbol::SymbolKind::Const {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::NotAssignable,
                            format!(
                                "cannot reassign to constant '{}'",
                                bind.interner.resolve(name)
                            ),
                        )
                        .with_range(range),
                    );
                }
            }
        }

        let value_ty = self.infer_type(value, bind);
        let is_empty_array_val = value_ty.is_dynamic()
            && matches!(&arena.expr(value).kind, varn_core::ast::ExprKind::Array { elements } if elements.is_empty());
        if !is_empty_array_val && !self.types_compatible_cached(&target_ty, &value_ty, Some(bind)) {
            let value_ty_s = value_ty.display(&self.ty_table, &bind.interner);
            let target_ty_s = target_ty.display(&self.ty_table, &bind.interner);
            self.emit(
                Diagnostic::error(
                    ErrorCode::TypeMismatch,
                    format!("type mismatch: cannot assign '{value_ty_s}' to '{target_ty_s}'"),
                )
                .with_range(range),
            );
        }
    }

    pub(super) fn check_update(&mut self, operand: ExprId, bind: &BindResult) {
        let arena = self.ast_arena;
        self.check_expr(operand, bind);
        if !matches!(
            &arena.expr(operand).kind,
            varn_core::ast::ExprKind::Identifier { .. } | varn_core::ast::ExprKind::Member { .. }
        ) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    "invalid left-hand side in update expression",
                )
                .with_range(arena.expr(operand).range),
            );
        }
    }
}
