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
    I8,
    I16,
    I32,
    U8,
    U16,
    U32,
    U64,
    F32,
    Span,
    TypedArray,
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
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::F32 => "f32",
            Self::Span => "Span",
            Self::TypedArray => "TypedArray",
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
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i32" => Some(Self::I32),
            "u8" => Some(Self::U8),
            "u16" => Some(Self::U16),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "f32" => Some(Self::F32),
            "Span" => Some(Self::Span),
            "TypedArray" => Some(Self::TypedArray),
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
                | Self::I8
                | Self::I16
                | Self::I32
                | Self::U8
                | Self::U16
                | Self::U32
                | Self::U64
                | Self::F32
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

/// How a value of a given static type occupies a class instance field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldRepr {
    pub size: u32,
    pub align: u32,
    /// Whether the collector must trace this field.
    pub is_gc_ref: bool,
}

impl TypeTag {
    /// The single authority on how a statically-typed field is laid out. The
    /// checker derives instance offsets from it while annotating field
    /// accesses, and the runtime derives `ClassLayout` from it; two tables
    /// would let a compiled access address a field the runtime placed
    /// elsewhere.
    ///
    /// The tag whose discriminant is `raw`, or `Dynamic` when `raw` names
    /// none — the conservative reading, and the one a truncated or
    /// forward-version operand must get.
    pub const fn from_u8(raw: u8) -> Self {
        if raw <= Self::TypedArray as u8 {
            // SAFETY: `TypeTag` is `#[repr(u8)]` with contiguous discriminants
            // from `Null = 0` through `TypedArray`, and `raw` is inside that range.
            unsafe { std::mem::transmute::<u8, Self>(raw) }
        } else {
            Self::Dynamic
        }
    }

    /// A tag with no unboxed representation falls back to a whole `VmValue`.
    pub const fn field_repr(self) -> FieldRepr {
        let (size, align, is_gc_ref) = match self {
            TypeTag::Bool | TypeTag::I8 | TypeTag::U8 => (1, 1, false),
            TypeTag::I16 | TypeTag::U16 => (2, 2, false),
            TypeTag::Char | TypeTag::I32 | TypeTag::U32 | TypeTag::F32 => (4, 4, false),
            TypeTag::Int | TypeTag::U64 | TypeTag::Float => (8, 8, false),
            TypeTag::Str
            | TypeTag::Array
            | TypeTag::Map
            | TypeTag::Set
            | TypeTag::Object
            | TypeTag::Class
            | TypeTag::Function
            | TypeTag::Task
            | TypeTag::Buffer
            | TypeTag::TypedArray
            | TypeTag::Generator => (8, 8, true),
            _ => (16, 8, true),
        };
        FieldRepr {
            size,
            align,
            is_gc_ref,
        }
    }
}

impl std::fmt::Display for TypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
