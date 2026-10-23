use super::context::{Builder, Result};
use crate::ssa::ir::{InstKind, Value};
use varn_tir::BackendTy;
use varn_tir::{Resolution, TirExpr};

impl<'m> Builder<'m> {
    pub(super) fn lower_field(
        &mut self,
        object: &TirExpr,
        name: &std::sync::Arc<str>,
        res: &Resolution,
        ty: BackendTy,
    ) -> Result<Value> {
        let obj = self.lower_expr(object)?;
        let kind = match res {
            Resolution::FieldSlot(slot) => match self.compact_field(object.ty, *slot) {
                Some((offset, kind)) => InstKind::GetFixedField {
                    object: obj,
                    slot: *slot,
                    offset,
                    tag: varn_core::FieldAccess::Compact(kind),
                },
                None => InstKind::GetProperty {
                    object: obj,
                    name: name.clone(),
                },
            },
            Resolution::StaticField(slot) => InstKind::GetFixedField {
                object: obj,
                slot: *slot,
                offset: 0,
                tag: varn_core::FieldAccess::Slot,
            },
            _ if name.as_ref() == varn_core::MemberKey::Length.as_str() => {
                if matches!(
                    object.ty.non_nullable(&self.tir.types),
                    varn_tir::BackendTy::Bytes
                ) {
                    return Ok(self.emit(InstKind::BytesLength { operand: obj }, BackendTy::Int));
                }
                let inner = match self.value_ty(obj) {
                    BackendTy::Nullable(id) => self.tir.types.get(id),
                    t => t,
                };
                match inner {
                    BackendTy::Str => {
                        return Ok(self.emit(InstKind::StrLength { operand: obj }, BackendTy::Int))
                    }
                    BackendTy::Array(_) | BackendTy::Tuple(_) => {
                        return Ok(self.emit(InstKind::ArrayLength { operand: obj }, BackendTy::Int))
                    }
                    BackendTy::Int
                    | BackendTy::Float
                    | BackendTy::Bool
                    | BackendTy::Char
                    | BackendTy::Bytes
                    | BackendTy::Enum(_)
                    | BackendTy::Fn(_)
                    | BackendTy::Decimal
                    | BackendTy::BigInt
                    | BackendTy::Void
                    | BackendTy::Never
                    | BackendTy::Dynamic(_)
                    | BackendTy::Map(..)
                    | BackendTy::Dict(_)
                    | BackendTy::Set(_)
                    | BackendTy::Class(_)
                    | BackendTy::Nullable(_) => InstKind::GetProperty {
                        object: obj,
                        name: name.clone(),
                    },
                }
            }
            Resolution::None
            | Resolution::Local(_)
            | Resolution::Param(_)
            | Resolution::Upvalue(_)
            | Resolution::GlobalSlot(_)
            | Resolution::NativeGlobal(_)
            | Resolution::ModuleSlot { .. }
            | Resolution::VtableSlot(_)
            | Resolution::DirectFn(_)
            | Resolution::Intrinsic(_)
            | Resolution::NativeOp(_)
            | Resolution::EnumVariant { .. }
            | Resolution::ByName { .. } => InstKind::GetProperty {
                object: obj,
                name: name.clone(),
            },
        };
        Ok(self.emit(kind, ty))
    }

    pub(super) fn lower_index(
        &mut self,
        object: &TirExpr,
        index: &TirExpr,
        ty: BackendTy,
    ) -> Result<Value> {
        let obj = self.lower_expr(object)?;
        let idx = self.lower_expr(index)?;
        let kind = if self.value_ty(obj) == BackendTy::Str && self.value_ty(idx) == BackendTy::Int {
            InstKind::CallNativeOp {
                object: obj,
                args: vec![idx],
                op_id: varn_core::runtime_ops::op_id(varn_core::runtime_ops::STR_INDEX),
            }
        } else if matches!(object.ty, varn_tir::BackendTy::Bytes)
            && matches!(self.value_ty(idx), BackendTy::Int | BackendTy::Dynamic(_))
        {
            InstKind::BytesGetIndex {
                object: obj,
                index: idx,
            }
        } else if matches!(
            self.value_ty(obj),
            BackendTy::Array(_) | BackendTy::Tuple(_)
        ) {
            InstKind::ArrayGetIndex {
                object: obj,
                index: idx,
            }
        } else if matches!(self.value_ty(obj), BackendTy::Map(..) | BackendTy::Dict(_)) {
            InstKind::MapGetIndex {
                object: obj,
                index: idx,
            }
        } else {
            InstKind::GetIndex {
                object: obj,
                index: idx,
            }
        };
        Ok(self.emit(kind, ty))
    }
}
