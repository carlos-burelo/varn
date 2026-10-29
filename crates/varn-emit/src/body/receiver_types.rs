use super::context::FnEmitter;
use varn_tir::{BackendTy, ClassInfo, DynReason};

impl<'a> FnEmitter<'a> {
    pub(super) fn class_of(&self, ty: BackendTy) -> Option<&'a ClassInfo> {
        match ty.non_nullable(self.tt) {
            BackendTy::Class(c) => self.m.classes.get(c.0 as usize),
            BackendTy::Literal(_)
            | BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => None,
        }
    }

    pub(super) fn core_class_name(&self, ty: BackendTy) -> Option<&'static str> {
        use varn_core::RuntimeKind as T;
        let tag = match ty.non_nullable(self.tt) {
            literal @ BackendTy::Literal(_) => {
                return self.core_class_name(literal.representation())
            }
            BackendTy::Array(_) => T::Array,
            BackendTy::Str => T::Str,
            BackendTy::Map(..) => T::Map,
            BackendTy::Set(_) => T::Set,
            BackendTy::Int => T::Int,
            BackendTy::Float => T::Float,
            BackendTy::Bool => T::Bool,
            BackendTy::Char => T::Char,
            BackendTy::Bytes => T::Bytes,
            BackendTy::Decimal => T::Decimal,
            BackendTy::BigInt => T::BigInt,
            BackendTy::Dynamic(DynReason::Yields(_)) => T::Generator,
            BackendTy::Dynamic(DynReason::Range(_)) => T::Range,
            BackendTy::Tuple(_)
            | BackendTy::Dict(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => return None,
        };
        varn_core::op_id::core_class_name(tag)
    }
}
