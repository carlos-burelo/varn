use super::context::FnEmitter;
use super::enum_variant::make_variant;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, Resolution, Span, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_identifier(
        &mut self,
        e: ExprId,
        name: varn_core::Atom,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let name_str = self.m.interner.resolve(name);
        let res = self.resolve_name(name_str);
        if matches!(res, Resolution::ByName { .. })
            && let Some((enum_id, tag)) = self.bare_variant(e, name_str)
        {
            return make_variant(enum_id, tag, Vec::new(), span);
        }
        TirExpr {
            kind: TirExprKind::Var,
            ty,
            res,
            span,
        }
    }
}
