use crate::RuntimeKind;

mod class;
pub use class::{ClassLayout, FieldLayout};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ScalarRepr {
    Bool,

    I64,

    F64,

    Ref,

    Boxed,
}

pub const COMPACT_REF_NULL: u64 = 0;

impl ScalarRepr {
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

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GcLayout {
    pub slots: Vec<GcSlot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GcSlot {
    pub offset: u32,

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
        assert_eq!(
            TypeLayout::of_field(Some(RuntimeKind::Class)).repr,
            ScalarRepr::Ref
        );
        assert_eq!(
            TypeLayout::of_field(Some(RuntimeKind::Str)).repr,
            ScalarRepr::Boxed
        );
        assert_eq!(TypeLayout::of_field(None).size, 16);
    }
}
