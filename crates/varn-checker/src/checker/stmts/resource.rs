use super::super::Checker;
use crate::binder::BindResult;
use varn_core::ast::VarDeclarator;
use varn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn check_using_stmt(
        &mut self,
        declarations: Vec<VarDeclarator>,
        is_await: bool,
        bind: &BindResult,
    ) {
        let dispose_method = if is_await { "disposeAsync" } else { "dispose" };
        let interface_name = if is_await {
            varn_core::well_known::ASYNC_DISPOSABLE
        } else {
            varn_core::well_known::DISPOSABLE
        };
        for d in &declarations {
            if d.init.is_none() {
                self.emit(
                    Diagnostic::error(
                        ErrorCode::ConstWithoutInitializer,
                        "'using' declaration must have an initializer",
                    )
                    .with_range(d.range),
                );
                continue;
            }
            let ann = d.type_ann.as_ref().or(match &d.id {
                varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                _ => None,
            });
            let ann_ty_opt = ann.map(|node| self.resolve_type_node_cached(node, bind));

            let init = d.init.unwrap();
            self.with_expected(ann_ty_opt, |c| c.check_expr(init, bind));
            let init_ty = self.infer_type(init, bind);

            if !init_ty.is_dynamic() && !self.member_exists_cached(&init_ty, dispose_method, bind) {
                let init_ty_s = init_ty.display(&self.ty_table, &bind.interner);
                self.emit(
                    Diagnostic::error(ErrorCode::InvalidUsingTarget, format!(
                        "type '{init_ty_s}' does not implement {interface_name}: missing '{dispose_method}()' method"
                    ))
                    .with_range(d.range),
                );
            }

            if let Some(ann_ty) = &ann_ty_opt {
                if !self.value_assignable_to(ann_ty, &init_ty, Some(init), Some(bind)) {
                    let ann_ty_s = ann_ty.display(&self.ty_table, &bind.interner);
                    let init_ty_s = init_ty.display(&self.ty_table, &bind.interner);
                    self.emit(
                        Diagnostic::error(ErrorCode::TypeMismatch, format!(
                            "type mismatch: declared as '{ann_ty_s}' but initialised with '{init_ty_s}'"
                        ))
                        .with_range(d.range),
                    );
                }
                self.check_pattern(&d.id, ann_ty, bind);
            } else {
                self.check_pattern(&d.id, &init_ty, bind);
            }
        }
    }
}
