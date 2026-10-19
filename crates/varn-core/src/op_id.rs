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
    GeneratorNext,
}

impl KnownOp {
    pub const ALL: [KnownOp; 12] = [
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
        KnownOp::GeneratorNext,
    ];

    pub const fn id(self) -> u64 {
        crate::runtime_ops::COUNT + self as u64
    }

    pub fn names(self) -> (RuntimeKind, &'static str) {
        match self {
            KnownOp::ArrayPush => (RuntimeKind::Array, crate::MemberKey::Push.as_str()),
            KnownOp::IntToString => (RuntimeKind::Int, crate::MemberKey::ToString.as_str()),
            KnownOp::StrStartsWith => (RuntimeKind::Str, crate::MemberKey::StartsWith.as_str()),
            KnownOp::StrEndsWith => (RuntimeKind::Str, crate::MemberKey::EndsWith.as_str()),
            KnownOp::StrSlice => (RuntimeKind::Str, crate::MemberKey::Slice.as_str()),
            KnownOp::StrIndexOf => (RuntimeKind::Str, crate::MemberKey::IndexOf.as_str()),
            KnownOp::StrIncludes => (RuntimeKind::Str, crate::MemberKey::Includes.as_str()),
            KnownOp::StrSplit => (RuntimeKind::Str, crate::MemberKey::Split.as_str()),
            KnownOp::StrSubstring => (RuntimeKind::Str, crate::MemberKey::Substring.as_str()),
            KnownOp::StrCharCodeAt => (RuntimeKind::Str, crate::MemberKey::CharCodeAt.as_str()),
            KnownOp::StrCodePointAt => (RuntimeKind::Str, crate::MemberKey::CodePointAt.as_str()),
            KnownOp::GeneratorNext => (RuntimeKind::Generator, crate::MemberKey::IterNext.as_str()),
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
