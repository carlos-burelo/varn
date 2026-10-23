use super::context::Builder;
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use varn_tir::{BackendTy, DynReason};

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
            None => match varn_core::abi::native_global_index(bare) {
                Some(native) => InstKind::LoadNativeGlobalIdx(native),
                None => InstKind::LoadGlobal(self.gname(bare)),
            },
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
        let conv = match to {
            BackendTy::BigInt if from == BackendTy::Int => varn_core::ValueConv::IntToBigInt,
            BackendTy::Decimal if from == BackendTy::Int => varn_core::ValueConv::IntToDecimal,
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::BigInt
            | BackendTy::Decimal
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => return v,
        };
        self.emit(
            InstKind::Convert { operand: v, conv },
            BackendTy::Dynamic(DynReason::Boxed),
        )
    }

    pub(super) fn coerce(&mut self, v: Value, target: BackendTy) -> Value {
        let from = self.value_ty(v);
        if from == target || (from.is_boxed() && target.is_boxed()) {
            v
        } else if from == BackendTy::Int && target == BackendTy::Float {
            self.emit(
                InstKind::Convert {
                    operand: v,
                    conv: varn_core::ValueConv::IntToFloat,
                },
                BackendTy::Float,
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
