//! The language's own type vocabulary (spec §13, §39), separate from
//! `TypeTag`, which classifies runtime values.

/// A primitive type of the language. `Void`, `Never` and `Dynamic` are the
/// special types: they classify no value of their own.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum LangPrimitive {
    Null,
    Bool,
    Int,
    Float,
    BigInt,
    Decimal,
    Char,
    Str,
    Void,
    Never,
    Dynamic,
}

impl LangPrimitive {
    pub const ALL: [Self; 11] = [
        Self::Null,
        Self::Bool,
        Self::Int,
        Self::Float,
        Self::BigInt,
        Self::Decimal,
        Self::Char,
        Self::Str,
        Self::Void,
        Self::Never,
        Self::Dynamic,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::BigInt => "bigint",
            Self::Decimal => "decimal",
            Self::Char => "char",
            Self::Str => "str",
            Self::Void => "void",
            Self::Never => "never",
            Self::Dynamic => "dynamic",
        }
    }

    pub fn from_str(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The runtime classification of this type's values.
    pub const fn runtime_tag(self) -> crate::TypeTag {
        use crate::TypeTag as T;
        match self {
            Self::Null => T::Null,
            Self::Bool => T::Bool,
            Self::Int => T::Int,
            Self::Float => T::Float,
            Self::BigInt => T::BigInt,
            Self::Decimal => T::Decimal,
            Self::Char => T::Char,
            Self::Str => T::Str,
            Self::Void => T::Void,
            Self::Never => T::Never,
            Self::Dynamic => T::Dynamic,
        }
    }

    /// Numeric domains (spec §2).
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::Int | Self::Float | Self::BigInt | Self::Decimal)
    }
}

impl std::fmt::Display for LangPrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// A platform collection named without type arguments (`Bytes`, a bare
/// `Map`): nominal types the checker knows structurally.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum BuiltinType {
    Array,
    Map,
    Set,
    Range,
    Bytes,
    Task,
    TaskHandle,
    Generator,
}

impl BuiltinType {
    pub const ALL: [Self; 8] = [
        Self::Array,
        Self::Map,
        Self::Set,
        Self::Range,
        Self::Bytes,
        Self::Task,
        Self::TaskHandle,
        Self::Generator,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Array => "Array",
            Self::Map => "Map",
            Self::Set => "Set",
            Self::Range => "Range",
            Self::Bytes => "Bytes",
            Self::Task => "Task",
            Self::TaskHandle => "TaskHandle",
            Self::Generator => "Generator",
        }
    }

    pub fn from_str(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name)
    }

    /// The runtime classification of this type's values.
    pub const fn runtime_tag(self) -> crate::TypeTag {
        use crate::TypeTag as T;
        match self {
            Self::Array => T::Array,
            Self::Map => T::Map,
            Self::Set => T::Set,
            Self::Range => T::Range,
            Self::Bytes => T::Bytes,
            Self::Task => T::Task,
            Self::TaskHandle => T::TaskHandle,
            Self::Generator => T::Generator,
        }
    }
}

impl std::fmt::Display for BuiltinType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for p in LangPrimitive::ALL {
            assert_eq!(LangPrimitive::from_str(p.name()), Some(p));
        }
        for b in BuiltinType::ALL {
            assert_eq!(BuiltinType::from_str(b.name()), Some(b));
        }
        assert_eq!(LangPrimitive::from_str("symbol"), None);
    }
}
