use super::context::FnEmitter;
use super::enum_variant::make_variant;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirExpr, TirExprKind};

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
        let ty = match (ty, &res) {
            (BackendTy::Dynamic(DynReason::Unrecorded), Resolution::NativeGlobal(idx)) => {
                native_global_ty(*idx)
            }
            (other, _) => other,
        };
        TirExpr {
            kind: TirExprKind::Var,
            ty,
            res,
            span,
        }
    }
}

fn native_global_ty(idx: u32) -> BackendTy {
    use varn_core::abi::{NativeValue, NATIVE_GLOBALS};
    match NATIVE_GLOBALS.get(idx as usize).map(|g| g.value) {
        Some(NativeValue::Function) => BackendTy::Dynamic(DynReason::Callable),
        Some(NativeValue::Class) => BackendTy::Dynamic(DynReason::Runtime),
        Some(NativeValue::Float) => BackendTy::Float,
        None => BackendTy::Dynamic(DynReason::Unrecorded),
    }
}
