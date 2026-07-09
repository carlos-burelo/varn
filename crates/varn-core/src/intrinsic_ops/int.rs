use super::wire::{encode, IntrinsicDomain};

/// `int` instance methods dispatched as intrinsics. `toString` formats the
/// NaN-boxed integer into a stack buffer in the VM — no intermediate `String`
/// allocation, and short results become SSO values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IntOp {
    ToString = 0x0,
}

impl IntOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Int, self as u8)
    }
}

/// `int` method name -> wire byte, for receivers statically typed `int`.
pub const METHOD_ENTRIES: &[(&str, u8)] = &[("toString", IntOp::ToString.wire())];
