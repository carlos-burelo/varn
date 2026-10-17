use core::mem::{align_of, offset_of, size_of};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiValue {
    pub tag: u64,
    pub payload: u64,
}

impl AbiValue {
    #[inline(always)]
    pub const fn from_raw_parts(tag: u64, payload: u64) -> Self {
        Self { tag, payload }
    }
    #[inline(always)]
    pub const fn null() -> Self {
        Self { tag: 0, payload: 0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiStacks {
    pub gpr: *mut i64,
    pub gpr_end: *mut i64,
    pub fpr: *mut f64,
    pub fpr_end: *mut f64,
    pub refs: *mut u32,
    pub refs_end: *mut u32,
    pub dyn_: *mut AbiValue,
    pub dyn_end: *mut AbiValue,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ActBases {
    pub bases: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiClosure {
    pub proto: *const u8,
    pub module_base: u32,
    pub ic_stride: u32,
    pub ic_entries: *const u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiFrame {
    pub closure: *const AbiClosure,
    pub caller: u32,
    pub resume: u32,
    pub dest: u16,
    pub _pad: u16,
    pub bases: ActBases,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiFrameArena {
    pub base: *mut AbiFrame,
    pub len: u32,
    pub cap: u32,
}

pub const NATIVE_GLOBALS: &[&str] = &[
    "print",
    "Array",
    "Bytes",
    "DivisionByZero",
    "Error",
    "Infinity",
    "IntegerOverflow",
    "Map",
    "MatchError",
    "NaN",
    "Range",
    "RangeError",
    "Set",
    "TypeError",
    "assertSummary",
    "bigint",
    "bool",
    "char",
    "debug",
    "decimal",
    "float",
    "input",
    "int",
    "isIsolate",
    "str",
];

pub fn native_global_index(name: &str) -> Option<u32> {
    NATIVE_GLOBALS
        .iter()
        .position(|n| *n == name)
        .map(|i| i as u32)
}

const _: () = {
    assert!(size_of::<AbiValue>() == 16, "AbiValue: 16 B tag+payload");
    assert!(align_of::<AbiValue>() == 8, "AbiValue: align 8");
    assert!(size_of::<AbiStacks>() == 64, "AbiStacks: 8 ptrs");
    assert!(size_of::<ActBases>() == 16, "ActBases: 4xu32");
    assert!(offset_of!(AbiFrame, closure) == 0, "AbiFrame.closure");
    assert!(offset_of!(AbiFrame, bases) == 20, "AbiFrame.bases");
    assert!(size_of::<AbiFrame>() == 40, "AbiFrame: tamaño fijo");
};
