#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum TypeTag {
    Null = 0,
    Bool,
    Int,
    Float,
    Str,
    BigInt,
    Decimal,
    Char,
    Symbol,
    Void,
    Never,
    Dynamic,
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
    NativeFn,
    Error,
    TypeError,
    RangeError,
    VmRef,
    TaskHandle,
    Buffer,
    Regex,
    DateTime,
    Duration,
    UUID,
}

impl TypeTag {
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
            Self::Symbol => "Symbol",
            Self::Void => "void",
            Self::Never => "never",
            Self::Dynamic => "dynamic",
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
            Self::NativeFn => "native_fn",
            Self::VmRef => "vm_ref",
            Self::TaskHandle => "TaskHandle",
            Self::Buffer => "Buffer",
            Self::Regex => "Regex",
            Self::DateTime => "DateTime",
            Self::Duration => "Duration",
            Self::UUID => "UUID",
            Self::Error => "Error",
            Self::TypeError => "TypeError",
            Self::RangeError => "RangeError",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "null" => Some(Self::Null),
            "bool" => Some(Self::Bool),
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "str" => Some(Self::Str),
            "bigint" => Some(Self::BigInt),
            "decimal" => Some(Self::Decimal),
            "char" => Some(Self::Char),
            "symbol" => Some(Self::Symbol),
            "void" => Some(Self::Void),
            "never" => Some(Self::Never),
            "dynamic" => Some(Self::Dynamic),
            "Array" => Some(Self::Array),
            "Map" => Some(Self::Map),
            "Set" => Some(Self::Set),
            "Tuple" => Some(Self::Tuple),
            "object" => Some(Self::Object),
            "class" => Some(Self::Class),
            "function" | "fn" => Some(Self::Function),
            "Generator" | "generator" => Some(Self::Generator),
            "Task" => Some(Self::Task),
            "Range" => Some(Self::Range),
            "Buffer" => Some(Self::Buffer),
            "enum" => Some(Self::Enum),
            _ => None,
        }
    }

    /// A tag whose values live inline in a `VmValue` word, with no heap
    /// identity: everything the checker treats as a scalar type.
    pub const fn is_primitive(self) -> bool {
        matches!(
            self,
            Self::Null
                | Self::Bool
                | Self::Int
                | Self::Float
                | Self::Char
                | Self::Void
                | Self::Never
                | Self::Str
                | Self::BigInt
                | Self::Decimal
                | Self::Symbol
        )
    }
}

pub trait VmValuePayload: std::fmt::Debug + std::any::Any {
    fn clone_payload(&self) -> Box<dyn VmValuePayload>;
    fn as_any(&self) -> &dyn std::any::Any;
}

impl Clone for Box<dyn VmValuePayload> {
    fn clone(&self) -> Self {
        self.clone_payload()
    }
}

impl std::fmt::Display for TypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
