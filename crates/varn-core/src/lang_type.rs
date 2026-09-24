//! The language's own type vocabulary (spec §13, §39), separate from
//! `RuntimeKind`, which classifies runtime values.

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

    /// The core module declaring this primitive's class (ADR-0018); `None`
    /// for a primitive without members.
    pub const fn core_module(self) -> Option<&'static str> {
        match self {
            Self::Bool => Some("core:types/bool"),
            Self::Int => Some("core:types/int"),
            Self::Float => Some("core:types/float"),
            Self::BigInt => Some("core:types/bigint"),
            Self::Decimal => Some("core:types/decimal"),
            Self::Char => Some("core:types/char"),
            Self::Str => Some("core:types/str"),
            Self::Null | Self::Void | Self::Never | Self::Dynamic => None,
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

    /// The core module declaring this type (ADR-0018).
    pub const fn core_module(self) -> &'static str {
        match self {
            Self::Array => "core:types/array",
            Self::Map => "core:types/map",
            Self::Set => "core:types/set",
            Self::Range => "core:types/range",
            Self::Bytes => "core:types/bytes",
            Self::Task | Self::TaskHandle | Self::Generator => "core:types/iteration",
        }
    }
}

impl std::fmt::Display for BuiltinType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// `true` when `name` is part of the language's type vocabulary rather than a
/// user or platform declaration.
pub fn is_lang_type_name(name: &str) -> bool {
    LangPrimitive::from_str(name).is_some() || BuiltinType::from_str(name).is_some()
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
