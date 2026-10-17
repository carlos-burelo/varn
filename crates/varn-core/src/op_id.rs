use crate::runtime_kind::RuntimeKind;

pub const CORE_MODULE: &str = "globals";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownOp {
    ArrayPush,
    IntToString,
    StrStartsWith,
    StrEndsWith,
    StrSlice,
    StrIndexOf,
    StrIncludes,
    StrSplit,
    StrSubstring,
    StrCharCodeAt,
    StrCodePointAt,
}

impl KnownOp {
    pub const ALL: [KnownOp; 11] = [
        KnownOp::ArrayPush,
        KnownOp::IntToString,
        KnownOp::StrStartsWith,
        KnownOp::StrEndsWith,
        KnownOp::StrSlice,
        KnownOp::StrIndexOf,
        KnownOp::StrIncludes,
        KnownOp::StrSplit,
        KnownOp::StrSubstring,
        KnownOp::StrCharCodeAt,
        KnownOp::StrCodePointAt,
    ];

    pub const fn id(self) -> u64 {
        crate::runtime_ops::COUNT + self as u64
    }

    pub fn names(self) -> (RuntimeKind, &'static str) {
        match self {
            KnownOp::ArrayPush => (RuntimeKind::Array, "push"),
            KnownOp::IntToString => (RuntimeKind::Int, "toString"),
            KnownOp::StrStartsWith => (RuntimeKind::Str, "startsWith"),
            KnownOp::StrEndsWith => (RuntimeKind::Str, "endsWith"),
            KnownOp::StrSlice => (RuntimeKind::Str, "slice"),
            KnownOp::StrIndexOf => (RuntimeKind::Str, "indexOf"),
            KnownOp::StrIncludes => (RuntimeKind::Str, "includes"),
            KnownOp::StrSplit => (RuntimeKind::Str, "split"),
            KnownOp::StrSubstring => (RuntimeKind::Str, "substring"),
            KnownOp::StrCharCodeAt => (RuntimeKind::Str, "charCodeAt"),
            KnownOp::StrCodePointAt => (RuntimeKind::Str, "codePointAt"),
        }
    }

    pub fn from_names(class: &str, method: &str) -> Option<KnownOp> {
        KnownOp::ALL.into_iter().find(|op| {
            let (kind, name) = op.names();
            kind.name() == class && name == method
        })
    }
}

pub const FIRST_REGISTRY_ID: u64 = crate::runtime_ops::COUNT + KnownOp::ALL.len() as u64;

pub const CORE_CLASSES: [RuntimeKind; 14] = [
    RuntimeKind::Array,
    RuntimeKind::Str,
    RuntimeKind::Bytes,
    RuntimeKind::Map,
    RuntimeKind::Set,
    RuntimeKind::Range,
    RuntimeKind::Generator,
    RuntimeKind::Symbol,
    RuntimeKind::Int,
    RuntimeKind::Float,
    RuntimeKind::Bool,
    RuntimeKind::Char,
    RuntimeKind::Decimal,
    RuntimeKind::BigInt,
];

#[inline]
pub fn is_core_class(tag: RuntimeKind) -> bool {
    CORE_CLASSES.contains(&tag)
}

pub fn core_class_name(tag: RuntimeKind) -> Option<&'static str> {
    is_core_class(tag).then(|| tag.name())
}

pub fn core_class_tag(name: &str) -> Option<RuntimeKind> {
    CORE_CLASSES
        .into_iter()
        .find(|&tag| core_class_name(tag) == Some(name))
}

pub fn core_class(name: &str) -> Option<&'static str> {
    core_class_tag(name).and_then(core_class_name)
}
