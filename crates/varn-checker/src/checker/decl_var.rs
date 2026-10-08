use super::recorder::Recorder;
use super::Checker;
use varn_core::ast::ExprKind;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn reject_void_value(&mut self, ty: &Type, range: varn_core::SourceRange) {
        if *ty == Type::Void {
            self.emit(
                Diagnostic::error(
                    ErrorCode::VoidValueUsed,
                    "a 'void' call produces no value; use `()` (Unit) for an empty result",
                )
                .with_range(range),
            );
        }
    }

    pub(super) fn check_variable(
        &mut self,
        rec: &mut Recorder,
        v: &varn_core::ast::VariableDecl,
        decl_range: &varn_core::SourceRange,
        bind: &BindResult,
    ) {
        for d in &v.declarators {
            let ann = d.type_ann.as_ref();
            let ann_ty_opt = ann.map(|node| self.resolve_type_node_cached(node, bind));

            if let Some(init_expr) = d.init {
                self.with_expected(ann_ty_opt, |c| c.check_expr(rec, init_expr, bind));

                if let Some(ann_ty) = &ann_ty_opt {
                    let init_ty = self.infer_type(rec, init_expr, bind);
                    let is_empty_array = init_ty.is_dynamic()
                        && matches!(&self.ast_arena.expr(init_expr).kind, ExprKind::Array { elements } if elements.is_empty());
                    let is_compatible =
                        self.value_assignable_to(ann_ty, &init_ty, Some(init_expr), Some(bind));
                    if !is_empty_array && !is_compatible {
                        let ann_ty_s = ann_ty.display(&self.ty_table, &bind.interner);
                        let init_ty_s = init_ty.display(&self.ty_table, &bind.interner);
                        self.emit(
                            Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                "type mismatch: declared as '{ann_ty_s}' but initialised with '{init_ty_s}'"
                            ))
                            .with_range(*decl_range),
                        );
                    }
                    self.check_pattern(rec, &d.id, ann_ty, bind);
                } else {
                    let init_ty = self.infer_type(rec, init_expr, bind);
                    self.reject_void_value(&init_ty, *decl_range);
                    let final_ty = if v.kind == varn_core::ast::VarKind::Let {
                        varn_binder::widen_literal(init_ty)
                    } else {
                        init_ty
                    };
                    self.check_pattern(rec, &d.id, &final_ty, bind);
                }
            }
        }
    }
}
