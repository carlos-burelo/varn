use super::wire::{encode, IntrinsicDomain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StringOp {
    Len      = 0x0,
    Contains = 0x1,
    StartsWith = 0x2,
    EndsWith   = 0x3,
    ToUpperCase = 0x4,
    ToLowerCase = 0x5,
    Trim     = 0x6,
}

impl StringOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::String, self as u8)
    }
}

pub const MAP_ENTRIES: &[(&str, u8)] = &[
    ("std:string/len",         StringOp::Len.wire()),
    ("std:string/contains",    StringOp::Contains.wire()),
    ("std:string/starts_with", StringOp::StartsWith.wire()),
    ("std:string/ends_with",   StringOp::EndsWith.wire()),
    ("std:string/to_uppercase", StringOp::ToUpperCase.wire()),
    ("std:string/to_lowercase", StringOp::ToLowerCase.wire()),
    ("std:string/trim",        StringOp::Trim.wire()),
];
