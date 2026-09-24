//! How a value occupies memory (spec §47–§48, §101): one table for size,
//! alignment and representation, read by the compiler (field offsets), the
//! runtime (instance payloads, the collector) and the JIT (compact field
//! access). Nobody re-derives a representation from a kind.

use varn_core::RuntimeKind;

/// The bytes a stored value is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ScalarRepr {
    /// `bool`, one byte.
    Bool,
    /// `int`, a raw `i64`.
    I64,
    /// `float`, a raw `f64`.
    F64,
    /// A heap reference as its 8-byte index; [`COMPACT_REF_NULL`] is `null`
    /// (the niche that makes `T?` over a reference free, spec §49).
    Ref,
    /// A whole two-word `VmValue` (tag + payload).
    Boxed,
}

/// The `null` niche of a [`ScalarRepr::Ref`] slot: no heap index is `u32::MAX`.
pub const COMPACT_REF_NULL: u64 = u32::MAX as u64;

impl ScalarRepr {
    /// Whether a slot of this representation can hold a GC reference.
    pub const fn holds_reference(self) -> bool {
        match self {
            ScalarRepr::Ref | ScalarRepr::Boxed => true,
            ScalarRepr::Bool | ScalarRepr::I64 | ScalarRepr::F64 => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TypeLayout {
    pub size: u32,
    pub align: u32,
    pub repr: ScalarRepr,
}

impl TypeLayout {
    const BOXED: Self = Self::new(16, 8, ScalarRepr::Boxed);

    const fn new(size: u32, align: u32, repr: ScalarRepr) -> Self {
        Self { size, align, repr }
    }

    /// The layout of a class field laid out by `kind`.
    ///
    /// A boxed field (`None`, or a kind with no unboxed form) is a whole
    /// `VmValue`. So are `str` — it can be `KIND_SSO`, an inline `VmValue`
    /// with no heap object — and `char`, which is always `HeapObj::Char` and
    /// would need heap access `InstanceData` does not have. Every other
    /// reference is always `KIND_HEAP`, so it compacts to an 8-byte index.
    pub const fn of_field(kind: Option<RuntimeKind>) -> Self {
        match kind {
            Some(RuntimeKind::Bool) => Self::new(1, 1, ScalarRepr::Bool),
            Some(RuntimeKind::Int) => Self::new(8, 8, ScalarRepr::I64),
            Some(RuntimeKind::Float) => Self::new(8, 8, ScalarRepr::F64),
            Some(
                RuntimeKind::Array
                | RuntimeKind::Map
                | RuntimeKind::Set
                | RuntimeKind::Object
                | RuntimeKind::Class
                | RuntimeKind::Function
                | RuntimeKind::Task
                | RuntimeKind::Bytes
                | RuntimeKind::Generator,
            ) => Self::new(8, 8, ScalarRepr::Ref),
            Some(
                RuntimeKind::Null
                | RuntimeKind::Str
                | RuntimeKind::Char
                | RuntimeKind::BigInt
                | RuntimeKind::Decimal
                | RuntimeKind::Symbol
                | RuntimeKind::Tuple
                | RuntimeKind::Range
                | RuntimeKind::Enum
                | RuntimeKind::Opaque
                | RuntimeKind::TaskHandle,
            )
            | None => Self::BOXED,
        }
    }
}

/// Where the references of a laid-out value are (spec §48): the collector
/// visits these slots and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GcLayout {
    pub slots: Vec<GcSlot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GcSlot {
    pub offset: u32,
    /// [`ScalarRepr::Ref`] or [`ScalarRepr::Boxed`].
    pub repr: ScalarRepr,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_hold_no_references() {
        for kind in [RuntimeKind::Bool, RuntimeKind::Int, RuntimeKind::Float] {
            assert!(!TypeLayout::of_field(Some(kind)).repr.holds_reference());
        }
        assert_eq!(TypeLayout::of_field(Some(RuntimeKind::Class)).repr, ScalarRepr::Ref);
        assert_eq!(TypeLayout::of_field(Some(RuntimeKind::Str)).repr, ScalarRepr::Boxed);
        assert_eq!(TypeLayout::of_field(None).size, 16);
    }
}
