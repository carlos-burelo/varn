use super::context::FnEmitter;
use varn_core::ast::{Arg, AstId, ExprId, ExprKind};
use varn_tir::{BackendTy, EnumId, Resolution, Span, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn enum_variant(&self, object: ExprId, variant: &str) -> Option<(EnumId, u16)> {
        let ExprKind::Identifier { name } = &self.ast_arena.expr(object).kind else {
            return None;
        };
        let name = self.m.interner.resolve(*name);
        let at = self.m.enums.iter().position(|info| {
            info.binding.as_deref() == Some(name)
                && info
                    .variant_global(variant)
                    .is_some_and(|global| self.m.globals.contains_key(&global))
        })?;
        self.variant_of(EnumId(at as u32), variant)
    }

    pub(super) fn bare_variant(&mut self, e: ExprId, variant: &str) -> Option<(EnumId, u16)> {
        let table = self.m.checker_table;
        let checked = self.expr_table.get(&e.index())?.ty;
        let value = if let varn_core::TypeKind::Fn(fid) = table.get(checked.0) {
            varn_sem::types::Type::resolved(table.get_function(fid).return_type)
        } else {
            checked
        };
        let lowered = crate::ty::lower_type(&value, table, self.m.interner, self.tt, self.m.names);
        let BackendTy::Enum(enum_id) = lowered else {
            return None;
        };
        self.variant_of(enum_id, variant)
    }

    pub(super) fn bare_variant_call(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        span: Span,
    ) -> Option<TirExpr> {
        let ExprKind::Identifier { name } = &self.ast_arena.expr(callee).kind else {
            return None;
        };
        let variant = self.m.interner.resolve(*name);
        if !matches!(self.resolve_name(variant), Resolution::ByName { .. }) {
            return None;
        }
        let (enum_id, tag) = self.bare_variant(callee, variant)?;
        let args = self.lower_call_args(call_id, args);
        Some(make_variant(enum_id, tag, args, span))
    }

    fn variant_of(&self, enum_id: EnumId, variant: &str) -> Option<(EnumId, u16)> {
        let info = self.m.enums.get(enum_id.0 as usize)?;
        let v = info.variants.iter().find(|v| v.name.as_ref() == variant)?;
        Some((enum_id, v.tag))
    }
}

pub(super) fn make_variant(
    enum_id: EnumId,
    tag: u16,
    args: Vec<varn_tir::TirArg>,
    span: Span,
) -> TirExpr {
    TirExpr {
        kind: TirExprKind::MakeVariant { args },
        ty: BackendTy::Enum(enum_id),
        res: Resolution::EnumVariant { enum_id, tag },
        span,
    }
}
