use super::wire::{encode, IntrinsicDomain};

/// Char-indexed `str` instance methods dispatched as intrinsics. The VM
/// implementation reads the heap string directly, so it can use the cached
/// ASCII state for O(1) byte addressing instead of per-call char scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StrOp {
    CharCodeAt = 0x0,
    CodePointAt = 0x1,
    Substring = 0x2,
    Slice = 0x3,
    Substr = 0x4,
    At = 0x5,
    IndexOf = 0x6,
    LastIndexOf = 0x7,
    StartsWith = 0x8,
    EndsWith = 0x9,
    Includes = 0xA,
    CharCode = 0xB,
    Split = 0xC,
}

impl StrOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Str, self as u8)
    }
}

/// `str` method name -> wire byte, for receivers statically typed `str`.
pub const METHOD_ENTRIES: &[(&str, u8)] = &[
    (
        crate::MemberKey::CharCodeAt.as_str(),
        StrOp::CharCodeAt.wire(),
    ),
    (
        crate::MemberKey::CodePointAt.as_str(),
        StrOp::CodePointAt.wire(),
    ),
    (
        crate::MemberKey::Substring.as_str(),
        StrOp::Substring.wire(),
    ),
    (crate::MemberKey::Slice.as_str(), StrOp::Slice.wire()),
    (crate::MemberKey::Substr.as_str(), StrOp::Substr.wire()),
    (crate::MemberKey::At.as_str(), StrOp::At.wire()),
    (crate::MemberKey::IndexOf.as_str(), StrOp::IndexOf.wire()),
    (
        crate::MemberKey::LastIndexOf.as_str(),
        StrOp::LastIndexOf.wire(),
    ),
    (
        crate::MemberKey::StartsWith.as_str(),
        StrOp::StartsWith.wire(),
    ),
    (crate::MemberKey::EndsWith.as_str(), StrOp::EndsWith.wire()),
    (crate::MemberKey::Includes.as_str(), StrOp::Includes.wire()),
    (crate::MemberKey::CharCode.as_str(), StrOp::CharCode.wire()),
    (crate::MemberKey::Split.as_str(), StrOp::Split.wire()),
];
