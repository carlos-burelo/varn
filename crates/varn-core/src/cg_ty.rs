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
}
