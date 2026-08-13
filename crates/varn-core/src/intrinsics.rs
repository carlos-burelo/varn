use crate::type_tag::TypeTag;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntrinsicType(pub TypeTag);

impl From<TypeTag> for IntrinsicType {
    fn from(tag: TypeTag) -> Self {
        Self(tag)
    }
}

#[allow(non_upper_case_globals)]
impl IntrinsicType {
    pub const Int: Self = Self(TypeTag::Int);
    pub const Float: Self = Self(TypeTag::Float);
    pub const Decimal: Self = Self(TypeTag::Decimal);
    pub const BigInt: Self = Self(TypeTag::BigInt);
    pub const Str: Self = Self(TypeTag::Str);
    pub const Char: Self = Self(TypeTag::Char);
    pub const Bool: Self = Self(TypeTag::Bool);
    pub const Symbol: Self = Self(TypeTag::Symbol);
    pub const Void: Self = Self(TypeTag::Void);
    pub const Null: Self = Self(TypeTag::Null);
    pub const Never: Self = Self(TypeTag::Never);
    pub const Dynamic: Self = Self(TypeTag::Dynamic);
    pub const Array: Self = Self(TypeTag::Array);
    pub const Map: Self = Self(TypeTag::Map);
    pub const Set: Self = Self(TypeTag::Set);
    pub const Task: Self = Self(TypeTag::Task);
    pub const TaskHandle: Self = Self(TypeTag::TaskHandle);
    pub const Generator: Self = Self(TypeTag::Generator);
    pub const Option: Self = Self(TypeTag::Enum);
    pub const Result: Self = Self(TypeTag::Enum);
    pub const Error: Self = Self(TypeTag::Error);
    pub const TypeError: Self = Self(TypeTag::TypeError);
    pub const RangeError: Self = Self(TypeTag::RangeError);
    pub const Range: Self = Self(TypeTag::Range);

    pub const fn as_str(self) -> &'static str {
        self.0.name()
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "decimal" => Some(Self::Decimal),
            "bigint" => Some(Self::BigInt),
            "str" => Some(Self::Str),
            "char" => Some(Self::Char),
            "bool" => Some(Self::Bool),
            // `Symbol` is the canonical class name; `symbol` is the surface
            // scalar annotation. Both resolve to the one tag.
            "Symbol" | "symbol" => Some(Self::Symbol),
            "void" => Some(Self::Void),
            "null" => Some(Self::Null),
            "never" => Some(Self::Never),
            "dynamic" => Some(Self::Dynamic),
            "Array" => Some(Self::Array),
            "Map" => Some(Self::Map),
            "Set" => Some(Self::Set),
            "Task" => Some(Self::Task),
            "TaskHandle" => Some(Self::TaskHandle),
            "Generator" => Some(Self::Generator),
            "Range" => Some(Self::Range),
            "Error" => Some(Self::Error),
            "TypeError" => Some(Self::TypeError),
            "RangeError" => Some(Self::RangeError),
            _ => None,
        }
    }

    pub fn is_scalar_primitive(self) -> bool {
        self.0.is_primitive()
    }
}

impl std::fmt::Display for IntrinsicType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemberKey {
    Tag,

    Value0,

    RawValue,

    Length,

    Callable,

    ToString,

    ValueOf,

    IterNext,

    IterDone,

    IterValue,
}

impl MemberKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tag => "__tag",
            Self::Value0 => "value0",
            Self::RawValue => "rawValue",
            Self::Length => "length",
            Self::Callable => "()",
            Self::ToString => "toString",
            Self::ValueOf => "valueOf",
            Self::IterNext => "next",
            Self::IterDone => "done",
            Self::IterValue => "value",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "__tag" => Some(Self::Tag),
            "value0" => Some(Self::Value0),
            "rawValue" => Some(Self::RawValue),
            "length" => Some(Self::Length),
            "()" => Some(Self::Callable),
            "toString" => Some(Self::ToString),
            "valueOf" => Some(Self::ValueOf),
            "next" => Some(Self::IterNext),
            "done" => Some(Self::IterDone),
            "value" => Some(Self::IterValue),
            _ => None,
        }
    }
}
