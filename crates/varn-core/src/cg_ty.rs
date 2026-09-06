//! Codegen-relevant projection of checker types.
//!
//! The checker's full `TypeKind` (generics, unions, mapped/conditional
//! types) is far richer than code generation can use. `CgTy` is the closed
//! vocabulary the backend understands: the checker projects each inferred
//! expression type down to a `CgTy` in `checker_annotations` and everything
//! past the checker (HIR value types, register metadata, the JIT) reasons
//! only in this vocabulary. Anything the projection cannot express is
//! `Dynamic` — never guessed.

use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CgTy {
    Int,
    Float,
    Bool,
    Str,
    Char,
    Decimal,
    BigInt,
    Array(Box<CgTy>),
    Map(Box<CgTy>, Box<CgTy>),
    Set(Box<CgTy>),
    /// Instance of a source-declared class, by name. Enough to gate
    /// fixed-field/vtable dispatch; cross-module identity is resolved by
    /// the consumer against its own class table.
    Class(Rc<str>),
    /// `T?` — the payload type plus null.
    Nullable(Box<CgTy>),
    Fn,
    Dynamic,
}

impl CgTy {
    /// The type with any nullability stripped, for consumers that guard
    /// null separately.
    pub fn non_nullable(&self) -> &CgTy {
        match self {
            CgTy::Nullable(inner) => inner.non_nullable(),
            other => other,
        }
    }

    /// The tag a consumer lays this type out by. `Nullable` keeps no unboxed
    /// representation of its own — it has to hold null too — so it answers
    /// `Dynamic` rather than its payload's tag.
    pub fn to_type_tag(&self) -> crate::TypeTag {
        use crate::TypeTag;
        match self {
            CgTy::Int => TypeTag::Int,
            CgTy::Float => TypeTag::Float,
            CgTy::Bool => TypeTag::Bool,
            CgTy::Str => TypeTag::Str,
            CgTy::Char => TypeTag::Char,
            CgTy::Decimal => TypeTag::Decimal,
            CgTy::BigInt => TypeTag::BigInt,
            CgTy::Array(_) => TypeTag::Array,
            CgTy::Map(_, _) => TypeTag::Map,
            CgTy::Set(_) => TypeTag::Set,
            CgTy::Class(_) => TypeTag::Class,
            CgTy::Fn => TypeTag::Function,
            CgTy::Nullable(_) | CgTy::Dynamic => TypeTag::Dynamic,
        }
    }
}
