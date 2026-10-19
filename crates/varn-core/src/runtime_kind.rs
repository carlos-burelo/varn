#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum RuntimeKind {
    Null = 0,
    Bool,
    Int,
    Float,
    Str,
    BigInt,
    Decimal,
    Char,
    Array,
    Map,
    Set,
    Tuple,
    Object,
    Class,
    Function,
    Generator,
    Task,
    Range,
    Enum,
    Opaque,
    TaskHandle,
    Bytes,
}

impl RuntimeKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Str => "str",
            Self::BigInt => "bigint",
            Self::Decimal => "decimal",
            Self::Char => "char",
            Self::Array => "Array",
            Self::Map => "Map",
            Self::Set => "Set",
            Self::Tuple => "Tuple",
            Self::Object => "object",
            Self::Class => "class",
            Self::Function => "function",
            Self::Generator => "Generator",
            Self::Task => "Task",
            Self::Range => "Range",
            Self::Enum => "enum",
            Self::Opaque => "opaque",
            Self::TaskHandle => "TaskHandle",
            Self::Bytes => "Bytes",
        }
    }
}

impl RuntimeKind {
    pub const fn from_u8(raw: u8) -> Option<Self> {
        if raw <= Self::Bytes as u8 {
            Some(unsafe { std::mem::transmute::<u8, Self>(raw) })
        } else {
            None
        }
    }

    pub const COUNT: usize = Self::Bytes as usize + 1;

    pub fn named(name: &str) -> Option<Self> {
        (0..Self::COUNT as u8)
            .filter_map(Self::from_u8)
            .find(|k| k.name() == name)
    }

    pub const fn encode(kind: Option<Self>) -> u8 {
        match kind {
            Some(k) => k as u8,
            None => u8::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FieldAccess {
    Slot,
    Compact(Option<RuntimeKind>),
}

impl FieldAccess {
    pub fn encode(self) -> u8 {
        match self {
            Self::Slot => 0,
            Self::Compact(kind) => {
                debug_assert_ne!(
                    kind,
                    Some(RuntimeKind::Null),
                    "a field is never laid out as null"
                );
                RuntimeKind::encode(kind)
            }
        }
    }

    pub const fn decode(raw: u8) -> Self {
        if raw == 0 {
            Self::Slot
        } else {
            Self::Compact(RuntimeKind::from_u8(raw))
        }
    }
}

impl std::fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
