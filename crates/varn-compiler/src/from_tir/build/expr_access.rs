use super::context::{Builder, Result};
use crate::ssa::ir::{InstKind, Value};
use crate::OptError;
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
        if let Resolution::EnumVariant { enum_id, tag } = res {
            return self.lower_variant_constant(*enum_id, *tag, ty);
        }
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

    fn lower_variant_constant(
        &mut self,
        enum_id: varn_tir::EnumId,
        tag: u16,
        ty: BackendTy,
    ) -> Result<Value> {
        let unsupported = || OptError::Unsupported("from_tir: variant constant without a slot");
        let info = self.tir.enum_info(enum_id).ok_or_else(unsupported)?;
        let global = info
            .variant_at(tag)
            .and_then(|variant| info.variant_global(&variant.name))
            .ok_or_else(unsupported)?;
        let slot = self.gslot(&global).ok_or_else(unsupported)?;
        Ok(self.emit(InstKind::LoadGlobalIdx(slot), ty))
    }

    pub(super) fn lower_length(&mut self, operand: &TirExpr, ty: BackendTy) -> Result<Value> {
        let obj = self.lower_expr(operand)?;
        if matches!(
            operand.ty.non_nullable(&self.tir.types),
            varn_tir::BackendTy::Bytes
        ) {
            return Ok(self.emit(InstKind::BytesLength { operand: obj }, ty));
        }
        let value_ty = self.value_ty(obj);
        let inner = if let BackendTy::Nullable(id) = value_ty {
            self.tir.types.get(id)
        } else {
            value_ty
        };
        let kind = match inner {
            BackendTy::Str => InstKind::StrLength { operand: obj },
            BackendTy::Array(_) | BackendTy::Tuple(_) => InstKind::ArrayLength { operand: obj },
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
                name: std::sync::Arc::from(varn_core::MemberKey::Length.as_str()),
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
