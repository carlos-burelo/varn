use core::mem::{align_of, offset_of, size_of};

use varn_core as _;

pub const ABI_MAGIC: u32 = 0x5641_524E;

pub const ABI_VERSION: u16 = 2;

pub const CLASS_GPR: usize = 0;
pub const CLASS_FPR: usize = 1;
pub const CLASS_REF: usize = 2;
pub const CLASS_DYN: usize = 3;

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
pub struct AbiHeader {
    pub magic: u32,
    pub version: u16,
    pub size: u16,
}

impl AbiHeader {
    pub const fn current() -> Self {
        Self {
            magic: ABI_MAGIC,
            version: ABI_VERSION,
            size: size_of::<AbiCtx>() as u16,
        }
    }
    #[inline(always)]
    pub const fn is_current(&self) -> bool {
        self.magic == ABI_MAGIC && self.version == ABI_VERSION
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

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiHeap {
    pub nursery_threshold: u64,
    pub gc_requested: u8,
    pub _pad: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AbiCtx {
    pub header: AbiHeader,
    pub epoch: u64,
    pub stacks: AbiStacks,
    pub frames: AbiFrameArena,
    pub heap: AbiHeap,
    pub result: AbiValue,
    pub poll: u8,
    pub _pad: [u8; 7],
}

impl AbiCtx {
    #[inline(always)]
    pub fn should_poll(&self) -> bool {
        self.poll != 0
    }
}

pub type RawJitFn = unsafe extern "C" fn(ctx: *mut AbiCtx, frame: *mut AbiFrame);

pub type InvokeDynamicFn =
    unsafe extern "C" fn(ctx: *mut AbiCtx, callee: AbiValue, argc: u32) -> AbiValue;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CallSite {
    pub pc_offset: u32,
    pub resume_ip: u32,
    pub dest: u16,
    pub _pad: u16,
}

impl CallSite {
    pub fn lookup(table: &[CallSite], pc_offset: u32) -> Option<(u32, u16)> {
        table
            .iter()
            .find(|e| e.pc_offset == pc_offset)
            .map(|e| (e.resume_ip, e.dest))
    }
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
    assert!(size_of::<AbiHeader>() == 8, "AbiHeader");
    assert!(offset_of!(AbiCtx, header) == 0, "AbiCtx.header");
    assert!(offset_of!(AbiCtx, epoch) == 8, "AbiCtx.epoch");
    assert!(offset_of!(AbiCtx, stacks) == 16, "AbiCtx.stacks");
    assert!(offset_of!(AbiCtx, result) % 8 == 0, "AbiCtx.result align");
    assert!(
        offset_of!(AbiCtx, poll) == offset_of!(AbiCtx, result) + 16,
        "AbiCtx.poll"
    );
    assert!(size_of::<AbiStacks>() == 64, "AbiStacks: 8 ptrs");
    assert!(size_of::<ActBases>() == 16, "ActBases: 4xu32");
    assert!(offset_of!(AbiFrame, closure) == 0, "AbiFrame.closure");
    assert!(offset_of!(AbiFrame, bases) == 20, "AbiFrame.bases");
    assert!(size_of::<AbiFrame>() == 40, "AbiFrame: tamaño fijo");
    assert!(size_of::<CallSite>() == 12, "CallSite");
    assert!(ABI_MAGIC == 0x5641_524E, "magic");
    assert!(ABI_VERSION == 2, "version");
};
