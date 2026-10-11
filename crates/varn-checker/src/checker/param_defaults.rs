use super::recorder::Recorder;
use super::Checker;
use varn_core::ast::Param;
use varn_core::{Diagnostic, ErrorCode};
use varn_sem::bind::BindResult;

impl Checker<'_> {
    pub(crate) fn check_param_defaults(
        &mut self,
        rec: &mut Recorder,
        params: &[Param],
        bind: &BindResult,
    ) {
        for param in params {
            let Some(default) = param.default else {
                continue;
            };
            let declared = param
                .type_ann
                .as_ref()
                .map(|ann| self.resolve_type_node_cached(ann, bind));
            self.with_expected(declared, |checker| {
                checker.check_expr(rec, default, bind);
                let Some(declared) = declared else {
                    return;
                };
                let given = checker.infer_type(rec, default, bind);
                if checker.value_fits(rec, &declared, &given, Some(default), Some(bind)) {
                    return;
                }
                let declared_s = declared.display(&checker.ty_table, &bind.interner);
                let given_s = given.display(&checker.ty_table, &bind.interner);
                let range = checker.ast_arena.expr(default).range;
                checker.emit(
                    Diagnostic::error(
                        ErrorCode::TypeMismatch,
                        format!(
                            "type mismatch: parameter is declared as '{declared_s}' but its default is '{given_s}'"
                        ),
                    )
                    .with_range(range),
                );
            });
        }
    }
}
