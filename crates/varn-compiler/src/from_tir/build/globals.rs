use super::context::Builder;
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use varn_tir::BackendTy;

impl<'m> Builder<'m> {
    pub(super) fn gname(&self, name: &str) -> Arc<str> {
        Arc::from(format!(
            "{}::{}",
            self.tir.source_file.replace('\\', "/"),
            name
        ))
    }

    pub(super) fn gslot(&self, bare: &str) -> Option<u32> {
        self.tir
            .global_names
            .iter()
            .position(|n| n.as_ref() == bare)
            .map(|i| i as u32)
    }

    pub(super) fn global_load(&self, bare: &str) -> InstKind {
        match self.gslot(bare) {
            Some(slot) => InstKind::LoadGlobalIdx(slot),
            None => InstKind::LoadGlobal(self.gname(bare)),
        }
    }

    pub(super) fn global_store(&self, bare: &str, value: Value) -> InstKind {
        match self.gslot(bare) {
            Some(slot) => InstKind::StoreGlobalIdx { slot, value },
            None => InstKind::StoreGlobal {
                name: self.gname(bare),
                value,
            },
        }
    }

    pub(super) fn widen_exact(&mut self, v: Value, from: BackendTy, to: BackendTy) -> Value {
        let conv = match (from, to) {
            (BackendTy::Int, BackendTy::BigInt) => varn_core::NumConv::IntToBigInt,
            (BackendTy::Int, BackendTy::Decimal) => varn_core::NumConv::IntToDecimal,
            _ => return v,
        };
        self.emit(InstKind::Convert { operand: v, conv }, HirType::Dynamic)
    }

    pub(super) fn coerce(&mut self, v: Value, target: HirType) -> Value {
        let from = self.value_ty(v);
        if from == target {
            v
        } else if from == HirType::Int && target == HirType::Float {
            self.emit(
                InstKind::Convert {
                    operand: v,
                    conv: varn_core::NumConv::IntToFloat,
                },
                HirType::Float,
            )
        } else {
            self.emit(
                InstKind::Cast {
                    operand: v,
                    ty: target,
                },
                target,
            )
        }
    }
}
