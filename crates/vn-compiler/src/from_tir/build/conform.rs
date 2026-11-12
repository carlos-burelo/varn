use super::context::{Builder, Result};
use crate::from_tir::describe::Describer;
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use vn_tir::BackendTy;
use vn_tir::DynReason;

pub(super) fn is_structural(ty: BackendTy) -> bool {
    matches!(
        ty,
        BackendTy::Literal(_)
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Dynamic(
                vn_tir::DynReason::Callable
                    | vn_tir::DynReason::Shape(_)
                    | vn_tir::DynReason::Interface(_)
            )
    )
}

impl<'m> Builder<'m> {
    pub(super) fn lower_conform(
        &mut self,
        v: Value,
        node_ty: BackendTy,
        ty: BackendTy,
        kind: vn_tir::CheckKind,
    ) -> Result<Value> {
        let op = match kind {
            vn_tir::CheckKind::Conform => vn_core::runtime_ops::CONFORM,
            vn_tir::CheckKind::Decode => vn_core::runtime_ops::DECODE,
        };
        let mut describer = Describer::new(self.tir);
        let desc = describer.describe(node_ty)?;
        let text = self.emit(InstKind::ConstStr(Arc::from(desc.encode())), BackendTy::Str);
        let mut args = vec![text];
        for name in &describer.witnesses {
            args.push(self.emit(
                self.global_load(name),
                BackendTy::Dynamic(DynReason::Runtime),
            ));
        }
        let checked = self.emit(
            InstKind::CallNativeOp {
                object: v,
                args,
                op_id: vn_core::runtime_ops::op_id(op),
            },
            BackendTy::Dynamic(DynReason::Boxed),
        );
        Ok(self.emit(
            InstKind::Cast {
                operand: checked,
                ty,
            },
            ty,
        ))
    }
}
