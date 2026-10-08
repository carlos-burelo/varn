use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use varn_tir::{Resolution, TirExpr};

impl<'m> Builder<'m> {
    pub(super) fn lower_field(
        &mut self,
        object: &TirExpr,
        name: &std::sync::Arc<str>,
        res: &Resolution,
        ty: HirType,
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
            _ if name.as_ref() == varn_core::MemberKey::Length.as_str() => {
                if matches!(
                    object.ty.non_nullable(&self.tir.types),
                    varn_tir::BackendTy::Bytes
                ) {
                    return Ok(self.emit(InstKind::BytesLength { operand: obj }, HirType::Int));
                }
                let inner = match self.value_ty(obj) {
                    HirType::Nullable(id) => self.ssa_types.get(id),
                    t @ HirType::Int
                    | t @ HirType::Float
                    | t @ HirType::Bool
                    | t @ HirType::Str
                    | t @ HirType::Ref
                    | t @ HirType::Dynamic
                    | t @ HirType::Array(_)
                    | t @ HirType::Map(..)
                    | t @ HirType::Set(_)
                    | t @ HirType::Class(_) => t,
                };
                match inner {
                    HirType::Str => {
                        return Ok(self.emit(InstKind::StrLength { operand: obj }, HirType::Int))
                    }
                    HirType::Array(_) => {
                        return Ok(self.emit(InstKind::ArrayLength { operand: obj }, HirType::Int))
                    }
                    HirType::Int
                    | HirType::Float
                    | HirType::Bool
                    | HirType::Ref
                    | HirType::Dynamic
                    | HirType::Map(..)
                    | HirType::Set(_)
                    | HirType::Class(_)
                    | HirType::Nullable(_) => InstKind::GetProperty {
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
            | Resolution::StaticField(_)
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
        ty: HirType,
    ) -> Result<Value> {
        let obj = self.lower_expr(object)?;
        let idx = self.lower_expr(index)?;
        let kind = if matches!(self.value_ty(obj), HirType::Array(_)) {
            InstKind::ArrayGetIndex {
                object: obj,
                index: idx,
            }
        } else if matches!(self.value_ty(obj), HirType::Map(_, _)) {
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
