use super::context::{Builder, Result};
use crate::from_tir::describe::Describer;
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use varn_tir::BackendTy;

pub(super) fn is_structural(ty: BackendTy) -> bool {
    matches!(
        ty,
        BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Dynamic(varn_tir::DynReason::Callable | varn_tir::DynReason::Shape(_))
    )
}

impl<'m> Builder<'m> {
    pub(super) fn lower_conform(
        &mut self,
        v: Value,
        node_ty: BackendTy,
        ty: HirType,
    ) -> Result<Value> {
        let mut describer = Describer::new(self.tir);
        let desc = describer.describe(node_ty);
        let conform = varn_core::abi::native_global_index(varn_core::abi::CONFORM_GLOBAL).ok_or(
            crate::OptError::Unsupported("from_tir: the conform runtime is not a native global"),
        )?;
        let callee = self.emit(InstKind::LoadNativeGlobalIdx(conform), HirType::Ref);
        let text = self.emit(InstKind::ConstStr(Arc::from(desc.encode())), HirType::Str);
        let mut args = vec![v, text];
        for name in &describer.witnesses {
            args.push(self.emit(self.global_load(name), HirType::Ref));
        }
        let checked = self.emit(InstKind::Call { callee, args }, HirType::Dynamic);
        Ok(self.emit(InstKind::Cast { operand: checked, ty }, ty))
    }
}
