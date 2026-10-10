use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use std::sync::Arc;
use varn_core::TypeDesc;
use varn_tir::BackendTy;

const DEPTH_LIMIT: usize = 32;

pub(super) fn is_structural(ty: BackendTy) -> bool {
    matches!(
        ty,
        BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Dynamic(varn_tir::DynReason::Callable)
    )
}

impl<'m> Builder<'m> {
    fn type_desc(&self, ty: BackendTy, witnesses: &mut Vec<Arc<str>>, depth: usize) -> TypeDesc {
        if depth >= DEPTH_LIMIT {
            return TypeDesc::Any;
        }
        let types = &self.tir.types;
        let of = |id: varn_tir::TyId, witnesses: &mut Vec<Arc<str>>| {
            if types.contains(id) {
                self.type_desc(types.get(id), witnesses, depth + 1)
            } else {
                TypeDesc::Any
            }
        };
        match ty {
            BackendTy::Int => TypeDesc::Int,
            BackendTy::Float => TypeDesc::Float,
            BackendTy::Bool => TypeDesc::Bool,
            BackendTy::Char => TypeDesc::Char,
            BackendTy::Str => TypeDesc::Str,
            BackendTy::Bytes => TypeDesc::Bytes,
            BackendTy::Decimal => TypeDesc::Decimal,
            BackendTy::BigInt => TypeDesc::BigInt,
            BackendTy::Array(el) => TypeDesc::Array(Box::new(of(el, witnesses))),
            BackendTy::Map(k, v) => {
                let key = of(k, witnesses);
                let val = of(v, witnesses);
                TypeDesc::Map(Box::new(key), Box::new(val))
            }
            BackendTy::Set(el) => TypeDesc::Set(Box::new(of(el, witnesses))),
            BackendTy::Tuple(list) => {
                if !types.contains_list(list) {
                    return TypeDesc::Any;
                }
                TypeDesc::Tuple(
                    types
                        .get_list(list)
                        .iter()
                        .map(|el| self.type_desc(*el, witnesses, depth + 1))
                        .collect(),
                )
            }
            BackendTy::Class(class) => match self.tir.class(class) {
                Some(info) => {
                    let witness = match witnesses.iter().position(|w| *w == info.name) {
                        Some(at) => at,
                        None => {
                            witnesses.push(info.name.clone());
                            witnesses.len() - 1
                        }
                    };
                    TypeDesc::Class {
                        witness: witness as u32,
                        name: info.name.to_string(),
                    }
                }
                None => TypeDesc::Any,
            },
            BackendTy::Enum(id) => match self.tir.enum_info(id) {
                Some(info) => TypeDesc::Enum(info.name.to_string()),
                None => TypeDesc::Any,
            },
            BackendTy::Fn(_) | BackendTy::Dynamic(varn_tir::DynReason::Callable) => {
                TypeDesc::Callable
            }
            BackendTy::Nullable(inner) => {
                let inner = of(inner, witnesses);
                if inner.is_any() {
                    TypeDesc::Any
                } else {
                    TypeDesc::Nullable(Box::new(inner))
                }
            }
            BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(
                varn_tir::DynReason::HostBoundary
                | varn_tir::DynReason::Union
                | varn_tir::DynReason::IndexSignature
                | varn_tir::DynReason::Declared
                | varn_tir::DynReason::NotYetSupported,
            ) => TypeDesc::Any,
        }
    }

    pub(super) fn lower_conform(
        &mut self,
        v: Value,
        node_ty: BackendTy,
        ty: HirType,
    ) -> Result<Value> {
        let mut witnesses = Vec::new();
        let desc = self.type_desc(node_ty, &mut witnesses, 0);
        let conform = varn_core::abi::native_global_index(varn_core::abi::CONFORM_GLOBAL).ok_or(
            crate::OptError::Unsupported("from_tir: the conform runtime is not a native global"),
        )?;
        let callee = self.emit(InstKind::LoadNativeGlobalIdx(conform), HirType::Ref);
        let text = self.emit(InstKind::ConstStr(Arc::from(desc.encode())), HirType::Str);
        let mut args = vec![v, text];
        for name in &witnesses {
            args.push(self.emit(self.global_load(name), HirType::Ref));
        }
        let checked = self.emit(InstKind::Call { callee, args }, HirType::Dynamic);
        Ok(self.emit(InstKind::Cast { operand: checked, ty }, ty))
    }
}
