//! The per-register physical kind the backend lowers against.
//!
//! Etapa 5: this is the ONLY type representation the JIT reads. It is a
//! projection of the TIR's `BackendTy` onto what codegen actually
//! discriminates — an unboxed scalar (`Int` / `Float` / `Bool`), a value that
//! may be a small-string inline payload (`Str`), a value that is always a heap
//! reference (`Ref`), or a value with no static shape (`Dynamic`). The class /
//! array / nullable type handles the checker carries never reached codegen, so
//! they are not kept here.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SlotKind {
    Int,
    Float,
    Bool,
    /// A `str` — either a heap string or an inline small-string payload, so
    /// NOT unconditionally a heap pointer.
    Str,
    /// Always a heap reference: a class instance, an array, a map/set, an
    /// enum value, a closure, a decimal/bigint/tuple. Never null, never a
    /// scalar, never inline.
    Ref,
    Dynamic,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RegisterMeta {
    pub kind: SlotKind,
}
