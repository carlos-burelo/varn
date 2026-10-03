use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use varn_tir::TirExpr;

impl<'m> Builder<'m> {
    pub(super) fn lower_range_lit(
        &mut self,
        start: &TirExpr,
        end: &TirExpr,
        inclusive: bool,
    ) -> Result<Value> {
        let s = self.lower_expr(start)?;
        let e2 = self.lower_expr(end)?;
        Ok(self.emit(
            InstKind::Range {
                start: s,
                end: e2,
                inclusive,
            },
            HirType::Ref,
        ))
    }

    pub(super) fn lower_object_rest(
        &mut self,
        object: &TirExpr,
        skip_keys: &[std::sync::Arc<str>],
    ) -> Result<Value> {
        let o = self.lower_expr(object)?;
        Ok(self.emit(
            InstKind::ObjectRest {
                object: o,
                skip_keys: skip_keys.to_vec(),
            },
            HirType::Ref,
        ))
    }

    pub(super) fn lower_extension_call(
        &mut self,
        func: &std::sync::Arc<str>,
        recv: &TirExpr,
        args: &[varn_tir::TirArg],
        ty: HirType,
    ) -> Result<Value> {
        let r = self.lower_expr(recv)?;
        let argv = self.lower_args(args)?;
        Ok(self.emit(
            InstKind::ExtensionCall {
                func: func.clone(),
                slot: self.gslot(func),
                recv: r,
                args: argv,
            },
            ty,
        ))
    }
}
