use super::wire::{encode, IntrinsicDomain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TypeCheckOp {
    IsInt   = 0x0,
    IsFloat = 0x1,
    IsBool  = 0x2,
    IsStr   = 0x3,
    IsNull  = 0x4,
}

impl TypeCheckOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::TypeCheck, self as u8)
    }
}

pub const MAP_ENTRIES: &[(&str, u8)] = &[
    ("std:types/is_int",   TypeCheckOp::IsInt.wire()),
    ("std:types/is_float", TypeCheckOp::IsFloat.wire()),
    ("std:types/is_bool",  TypeCheckOp::IsBool.wire()),
    ("std:types/is_str",   TypeCheckOp::IsStr.wire()),
    ("std:types/is_null",  TypeCheckOp::IsNull.wire()),
];
