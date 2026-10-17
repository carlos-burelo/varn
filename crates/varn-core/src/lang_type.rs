use crate::Atom;

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

    const IDENTITY: [(Atom, Option<Atom>, Self); 11] = {
        let mut out = [(Atom::of_const(""), None, Self::Null); 11];
        let mut i = 0;
        while i < Self::ALL.len() {
            let p = Self::ALL[i];
            let module = match p.core_module() {
                Some(m) => Some(Atom::of_const(m)),
                None => None,
            };
            out[i] = (Atom::of_const(p.name()), module, p);
            i += 1;
        }
        out
    };

    pub fn identify(name: Atom, origin: Option<Atom>) -> Option<Self> {
        let origin = origin?;
        Self::IDENTITY
            .iter()
            .find(|(n, m, _)| *n == name && *m == Some(origin))
            .map(|&(_, _, p)| p)
    }

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

    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::Int | Self::Float | Self::BigInt | Self::Decimal)
    }
}

impl std::fmt::Display for LangPrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

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
    AsyncGenerator,
}

impl BuiltinType {
    pub const ALL: [Self; 9] = [
        Self::Array,
        Self::Map,
        Self::Set,
        Self::Range,
        Self::Bytes,
        Self::Task,
        Self::TaskHandle,
        Self::Generator,
        Self::AsyncGenerator,
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
            Self::AsyncGenerator => "AsyncGenerator",
        }
    }

    const IDENTITY: [(Atom, Atom, Self); 9] = {
        let mut out = [(Atom::of_const(""), Atom::of_const(""), Self::Array); 9];
        let mut i = 0;
        while i < Self::ALL.len() {
            let b = Self::ALL[i];
            out[i] = (Atom::of_const(b.name()), Atom::of_const(b.core_module()), b);
            i += 1;
        }
        out
    };

    pub fn identify(name: Atom, origin: Option<Atom>) -> Option<Self> {
        let origin = origin?;
        Self::IDENTITY
            .iter()
            .find(|(n, m, _)| *n == name && *m == origin)
            .map(|&(_, _, b)| b)
    }

    pub const fn core_module(self) -> &'static str {
        match self {
            Self::Array => "core:types/array",
            Self::Map => "core:types/map",
            Self::Set => "core:types/set",
            Self::Range => "core:types/range",
            Self::Bytes => "core:types/bytes",
            Self::Task | Self::TaskHandle | Self::Generator | Self::AsyncGenerator => {
                "core:types/iteration"
            }
        }
    }
}

impl std::fmt::Display for BuiltinType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoreSum {
    Option,
    Result,
}

impl CoreSum {
    pub const ALL: [Self; 2] = [Self::Option, Self::Result];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Option => "Option",
            Self::Result => "Result",
        }
    }

    pub const fn module(self) -> &'static str {
        match self {
            Self::Option => "core:types/option",
            Self::Result => "core:types/result",
        }
    }

    pub const fn failure_variant(self) -> &'static str {
        match self {
            Self::Option => "None",
            Self::Result => "Err",
        }
    }

    pub const fn success_variant(self) -> &'static str {
        match self {
            Self::Option => "Some",
            Self::Result => "Ok",
        }
    }

    pub fn identify(name: Atom, origin: Option<Atom>) -> Option<Self> {
        let origin = origin?;
        Self::ALL
            .into_iter()
            .find(|s| Atom::of_const(s.name()) == name && Atom::of_const(s.module()) == origin)
    }

    pub fn named(name: Atom) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|s| Atom::of_const(s.name()) == name)
    }
}

pub fn is_lang_type(name: Atom, origin: Option<Atom>) -> bool {
    LangPrimitive::identify(name, origin).is_some() || BuiltinType::identify(name, origin).is_some()
}
