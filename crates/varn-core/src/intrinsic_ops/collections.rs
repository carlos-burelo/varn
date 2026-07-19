use super::wire::{encode, IntrinsicDomain};

/// Hot `Map` instance methods dispatched as intrinsics: the VM
/// implementation reaches the backing table directly on the heap — no
/// argument marshalling, no contract wrapper, no fat-value round trip.
/// Cold methods (`keys`/`values`/`entries`/`forEach`) stay on the native
/// op-id path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MapOp {
    Get = 0x0,
    Set = 0x1,
    Has = 0x2,
    Delete = 0x3,
    Clear = 0x4,
}

impl MapOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Map, self as u8)
    }
}

pub const MAP_METHOD_ENTRIES: &[(&str, u8)] = &[
    ("get", MapOp::Get.wire()),
    ("set", MapOp::Set.wire()),
    ("has", MapOp::Has.wire()),
    ("delete", MapOp::Delete.wire()),
    ("clear", MapOp::Clear.wire()),
];

/// Hot `Set` instance methods; same rationale as [`MapOp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SetOp {
    Add = 0x0,
    Has = 0x1,
    Delete = 0x2,
    Clear = 0x3,
}

impl SetOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Set, self as u8)
    }
}

pub const SET_METHOD_ENTRIES: &[(&str, u8)] = &[
    ("add", SetOp::Add.wire()),
    ("has", SetOp::Has.wire()),
    ("delete", SetOp::Delete.wire()),
    ("clear", SetOp::Clear.wire()),
];
