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
}

impl StrOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Str, self as u8)
    }
}

/// `str` method name -> wire byte, for receivers statically typed `str`.
pub const METHOD_ENTRIES: &[(&str, u8)] = &[
    ("charCodeAt", StrOp::CharCodeAt.wire()),
    ("codePointAt", StrOp::CodePointAt.wire()),
    ("substring", StrOp::Substring.wire()),
    ("slice", StrOp::Slice.wire()),
    ("substr", StrOp::Substr.wire()),
    ("at", StrOp::At.wire()),
    ("indexOf", StrOp::IndexOf.wire()),
    ("lastIndexOf", StrOp::LastIndexOf.wire()),
    ("startsWith", StrOp::StartsWith.wire()),
    ("endsWith", StrOp::EndsWith.wire()),
    ("includes", StrOp::Includes.wire()),
    ("contains", StrOp::Includes.wire()),
    ("charCode", StrOp::CharCode.wire()),
];
