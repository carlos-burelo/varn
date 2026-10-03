use crate::binder::BindResult;
use crate::checker::Checker;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::source::SourceRange;

use super::super::members::extension_type_name;

impl<'r> Checker<'r> {
    pub(super) fn record_extension_call(
        &mut self,
        callee: ExprId,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &arena.expr(callee).kind
        else {
            return;
        };
        let (object, property) = (*object, *property);
        let ExprKind::Identifier { name: method_name } = &arena.expr(property).kind else {
            return;
        };
        let obj_ty_raw = self.infer_type(object, bind);
        let obj_ty = obj_ty_raw.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let Some(tn) = extension_type_name(self, &obj_ty, &self.ty_table, bind) else {
            return;
        };
        let Some(method_map) = bind.extensions.methods.get(tn.as_ref()) else {
            return;
        };
        if let Some(mangled) = method_map.get(bind.interner.resolve(*method_name)) {
            self.desugar
                .extension_calls
                .insert(range.start.offset, mangled.clone());
        }
    }
}
