mod alloc;
mod blocks;
mod mark;
mod native;
mod osmem;
mod stats;
mod sweep;
mod typed;

use super::obj::HeapObj;
use blocks::{block_of, cells_span, large_layout, Block, SizeClass};
use std::alloc::{alloc_zeroed, dealloc};
use std::ptr::NonNull;
use vn_types::cell::{
    cells_per_block, class_for, CELL_CLASSES, CELL_CLASS_LARGE, HEADER_BYTES, HEADER_CLASS_OFF,
    HEADER_KIND_OFF, HEADER_STATE_OFF,
};
use vn_types::HeapRef;

const MAJOR_MARK: u8 = 0x80;
const MINOR_MARK: u8 = 0x40;
const TRACED: u8 = 0x20;
const NEEDS_DROP: u8 = vn_types::cell::STATE_NEEDS_DROP;
const SIDE_TABLE: u8 = vn_types::cell::STATE_SIDE_TABLE;
const STATE_MASK: u8 = 0x07;

const _: () = assert!(HEADER_BYTES == 8);
const _: () = assert!(HEADER_BYTES == std::mem::size_of::<ObjHeader>());
const _: () = assert!(HEADER_STATE_OFF == std::mem::offset_of!(ObjHeader, state));
const _: () = assert!(HEADER_KIND_OFF == std::mem::offset_of!(ObjHeader, kind));
const _: () = assert!(HEADER_CLASS_OFF == std::mem::offset_of!(ObjHeader, class));

#[repr(C)]
pub(in crate::heap) struct ObjHeader {
    pub(in crate::heap) state: u8,
    pub(in crate::heap) kind: u8,
    pub(in crate::heap) class: u8,
    pub(in crate::heap) _pad: u8,
    pub(in crate::heap) _aux: u32,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SlotState {
    Free = 0,
    Old = 1,
    Remembered = 2,
    Young = 3,
    Survivor = 4,
}

impl SlotState {
    #[inline(always)]
    fn from_byte(b: u8) -> Self {
        match b & STATE_MASK {
            1 => Self::Old,
            2 => Self::Remembered,
            3 => Self::Young,
            4 => Self::Survivor,
            _ => Self::Free,
        }
    }
}

pub(crate) enum SweptInterned {
    Str(vn_types::RuntimeString),
    Char(char),
    BigInt(Box<num_bigint::BigInt>),
    Decimal(Box<bigdecimal::BigDecimal>),
}

#[inline(always)]
fn instance_has_references(r: HeapRef) -> bool {
    let inst = unsafe {
        vn_types::value::InstanceRef::from_data_ptr(
            (r.addr() as usize + vn_types::cell::INST_CELL_DATA_OFF) as *mut u8,
        )
    };
    inst.has_gc_references()
}

#[inline(always)]
pub(in crate::heap) fn header<'a>(r: HeapRef) -> &'a mut ObjHeader {
    unsafe { &mut *r.as_ptr::<ObjHeader>() }
}

#[inline(always)]
pub(in crate::heap) fn body<T>(r: HeapRef) -> *mut T {
    (r.addr() as usize + HEADER_BYTES) as *mut T
}

#[inline(always)]
fn set_age(r: HeapRef, age: SlotState) {
    let h = header(r);
    h.state = (h.state & !(STATE_MASK | MINOR_MARK)) | age as u8;
}

#[inline(always)]
fn block_ref(block: NonNull<Block>) -> &'static Block {
    unsafe { &*block.as_ptr() }
}

#[inline(always)]
fn block_ref_mut(block: NonNull<Block>) -> &'static mut Block {
    unsafe { &mut *block.as_ptr() }
}

pub(crate) struct CellSpace {
    pub(crate) classes: Vec<SizeClass>,
    large: Vec<(HeapRef, usize)>,
    pub(crate) native: bool,
    pub(in crate::heap) native_roots: Vec<HeapRef>,
    pub(crate) old_births: u64,
    pub(crate) old_bytes: u64,
    pub(crate) nursery_bytes: u64,
    pub(crate) swept_freed: u64,
    pub(crate) swept_identities: Vec<usize>,
    pub(crate) swept_interned: Vec<SweptInterned>,
    pub(crate) check_allocs: bool,
    pub(crate) abandon_objects: bool,
}

impl Default for CellSpace {
    fn default() -> Self {
        Self {
            classes: CELL_CLASSES.iter().map(|_| SizeClass::new()).collect(),
            large: Vec::new(),
            native: false,
            native_roots: Vec::new(),
            old_births: 0,
            old_bytes: 0,
            nursery_bytes: 0,
            swept_freed: 0,
            swept_identities: Vec::new(),
            swept_interned: Vec::new(),
            check_allocs: false,
            abandon_objects: false,
        }
    }
}

impl CellSpace {
    #[inline(always)]
    pub(crate) fn get(&self, r: HeapRef) -> &HeapObj {
        debug_assert_ne!(self.state(r), SlotState::Free, "reference to a freed cell");
        debug_assert_ne!(
            header(r).kind,
            vn_types::cell::CELL_KIND_INSTANCE,
            "colocated instance has no HeapObj; use instance()"
        );
        unsafe { &*body::<HeapObj>(r) }
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, r: HeapRef) -> &mut HeapObj {
        debug_assert_ne!(self.state(r), SlotState::Free, "reference to a freed cell");
        debug_assert_ne!(
            header(r).kind,
            vn_types::cell::CELL_KIND_INSTANCE,
            "colocated instance has no HeapObj; use instance()"
        );
        unsafe { &mut *body::<HeapObj>(r) }
    }

    #[inline(always)]
    pub(crate) fn state(&self, r: HeapRef) -> SlotState {
        SlotState::from_byte(header(r).state)
    }

    #[inline(always)]
    pub(crate) fn header_kind(&self, r: HeapRef) -> u8 {
        header(r).kind
    }

    #[inline(always)]
    pub(crate) fn set_state(&mut self, r: HeapRef, state: SlotState) {
        let h = header(r);
        h.state = (h.state & !STATE_MASK) | state as u8;
    }

    #[inline(always)]
    pub(crate) fn cell_bytes(&self, r: HeapRef) -> u64 {
        let h = header(r);
        if h.class == CELL_CLASS_LARGE {
            h._aux as u64
        } else {
            CELL_CLASSES[h.class as usize] as u64
        }
    }

    fn record_dead(&mut self, r: HeapRef) {
        use super::obj::DeadEntry;
        if header(r).kind == vn_types::cell::CELL_KIND_INSTANCE {
            return;
        }
        match self.get(r).dead_entry() {
            Some(DeadEntry::Identity(key)) => self.swept_identities.push(key),
            Some(DeadEntry::Interned(key)) => self.swept_interned.push(key),
            None => {}
        }
    }
}
