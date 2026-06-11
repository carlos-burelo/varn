use super::wire::{encode, IntrinsicDomain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArrayOp {
    Len    = 0x0,
    Push   = 0x1,
    Pop    = 0x2,
    Contains = 0x3,
}

impl ArrayOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Array, self as u8)
    }
}

pub const MAP_ENTRIES: &[(&str, u8)] = &[
    ("std:array/len",      ArrayOp::Len.wire()),
    ("std:array/push",     ArrayOp::Push.wire()),
    ("std:array/pop",      ArrayOp::Pop.wire()),
    ("std:array/contains", ArrayOp::Contains.wire()),
];
