#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    AsyncQueue,
    TaskHandle,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TypeFlags: u32 {
        const NONE = 0;
        const IS_SCALAR = 1 << 0;
        const IS_REFERENCE = 1 << 1;
        const IS_CALLABLE = 1 << 2;
        const IS_ITERABLE = 1 << 3;
        const IS_ASYNC = 1 << 4;
        const HAS_VTABLE = 1 << 5;
    }
}

pub struct BaseType {
    pub tag: TypeTag,
    pub name: &'static str,
    pub flags: TypeFlags,
}

impl TypeTag {
    pub fn name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Str => "str",
            Self::BigInt => "bigint",
            Self::Decimal => "decimal",
            Self::Char => "char",
            Self::Symbol => "symbol",
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
            Self::AsyncQueue => "AsyncQueue",
            Self::TaskHandle => "TaskHandle",
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
            "enum" => Some(Self::Enum),
            _ => None,
        }
    }

    pub fn flags(self) -> TypeFlags {
        match self {
            Self::Null
            | Self::Bool
            | Self::Int
            | Self::Float
            | Self::Char
            | Self::Void
            | Self::Never => TypeFlags::IS_SCALAR,
            Self::Str | Self::BigInt | Self::Decimal | Self::Symbol => {
                TypeFlags::IS_SCALAR | TypeFlags::HAS_VTABLE
            }
            Self::Array
            | Self::Map
            | Self::Set
            | Self::Tuple
            | Self::Object
            | Self::Class
            | Self::Generator
            | Self::Range
            | Self::Enum => TypeFlags::IS_REFERENCE | TypeFlags::HAS_VTABLE,
            Self::Function | Self::NativeFn => {
                TypeFlags::IS_REFERENCE | TypeFlags::IS_CALLABLE | TypeFlags::HAS_VTABLE
            }
            Self::Task | Self::TaskHandle => {
                TypeFlags::IS_REFERENCE | TypeFlags::IS_ASYNC | TypeFlags::HAS_VTABLE
            }
            Self::VmRef | Self::AsyncQueue => TypeFlags::IS_REFERENCE,
            Self::Error | Self::TypeError | Self::RangeError => {
                TypeFlags::IS_REFERENCE | TypeFlags::HAS_VTABLE
            }
            Self::Dynamic => TypeFlags::NONE,
        }
    }

    pub fn to_intrinsic_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Array => "Array",
            Self::Map => "Map",
            Self::Set => "Set",
            Self::Task => "Task",
            Self::Generator => "Generator",
            Self::Range => "Range",
            _ => self.name(),
        }
    }

    pub fn to_runtime_str(self) -> &'static str {
        match self {
            Self::Array => "array",
            Self::Object => "object",
            Self::Function | Self::NativeFn => "fn",
            Self::Class => "class",
            Self::Generator => "generator",
            Self::AsyncQueue => "asyncqueue",
            Self::Enum => "enum",
            _ => self.name(),
        }
    }

    pub fn is_primitive(self) -> bool {
        (self.flags() & TypeFlags::IS_SCALAR).bits() != 0
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
