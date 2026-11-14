use super::osmem;
use std::alloc::Layout;
use std::ptr::NonNull;
use vn_types::cell::{cells_per_block, BLOCK_BYTES, BLOCK_HEAD_BYTES, CELL_ALIGN, MAX_MARK_WORDS};
use vn_types::HeapRef;

pub(super) fn large_layout(bytes: usize) -> Layout {
    Layout::from_size_align(bytes, CELL_ALIGN).expect("large cell layout")
}

#[repr(C)]
pub(crate) struct Block {
    pub(crate) class: u8,
    pub(crate) swept: u8,
    pub(crate) _pad: u16,
    pub(crate) used: u32,
    pub(crate) live: u32,
    pub(crate) free_head: u64,
    raw: u64,
    pub(crate) marks: [u64; MAX_MARK_WORDS],
}

const _: () = assert!(std::mem::size_of::<Block>() <= BLOCK_HEAD_BYTES);
const _: () = assert!(BLOCK_HEAD_BYTES.is_multiple_of(CELL_ALIGN));

impl Block {
    pub(super) fn alloc(class: u8) -> NonNull<Block> {
        let (ptr, base) = osmem::map_block();
        let block = ptr.cast::<Block>();
        unsafe {
            let block = &mut *block.as_ptr();
            block.class = class;
            block.swept = 1;
            block.raw = base.as_ptr() as u64;
        }
        block
    }

    pub(super) fn free(block: NonNull<Block>) {
        let base = block_ref_raw(block).raw;
        osmem::unmap_block(unsafe { NonNull::new_unchecked(base as *mut u8) });
    }

    #[inline(always)]
    pub(super) fn base(&self) -> u64 {
        self as *const Block as u64
    }

    #[inline(always)]
    pub(super) fn cells_start(&self) -> u64 {
        self.base() + BLOCK_HEAD_BYTES as u64
    }

    #[inline(always)]
    pub(super) fn cell_at(&self, index: usize, cell: usize) -> HeapRef {
        unsafe { HeapRef::from_addr_unchecked(self.cells_start() + (index * cell) as u64) }
    }

    #[inline(always)]
    pub(super) fn mark(&mut self, index: usize) -> bool {
        let word = &mut self.marks[index / 64];
        let bit = 1u64 << (index % 64);
        if *word & bit != 0 {
            return false;
        }
        *word |= bit;
        true
    }

    #[inline(always)]
    pub(super) fn is_marked(&self, index: usize) -> bool {
        self.marks[index / 64] & (1u64 << (index % 64)) != 0
    }

    pub(super) fn clear_marks(&mut self) {
        self.marks.fill(0);
    }
}

#[inline(always)]
pub(super) fn block_of(addr: u64) -> NonNull<Block> {
    unsafe { NonNull::new_unchecked((addr & !(BLOCK_BYTES as u64 - 1)) as *mut Block) }
}

#[inline(always)]
fn block_ref_raw(block: NonNull<Block>) -> &'static Block {
    unsafe { &*block.as_ptr() }
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct AllocLane {
    pub(crate) free: u64,
    pub(crate) bump: u64,
    pub(crate) end: u64,
}

const _: () = assert!(std::mem::offset_of!(AllocLane, free) == vn_types::cell::LANE_FREE_OFF);
const _: () = assert!(std::mem::offset_of!(AllocLane, bump) == vn_types::cell::LANE_BUMP_OFF);
const _: () = assert!(std::mem::offset_of!(AllocLane, end) == vn_types::cell::LANE_END_OFF);
const _: () = assert!(std::mem::size_of::<AllocLane>() == vn_types::cell::LANE_SIZE);

pub(crate) struct SizeClass {
    pub(super) blocks: Vec<NonNull<Block>>,
    pub(crate) lane: AllocLane,
    pub(crate) active: Option<NonNull<Block>>,
}

impl SizeClass {
    pub(super) fn new() -> Self {
        Self {
            blocks: Vec::new(),
            lane: AllocLane::default(),
            active: None,
        }
    }
}

const _: () = assert!(std::mem::offset_of!(SizeClass, lane) == vn_types::cell::SIZE_CLASS_LANE_OFF);
const _: () = assert!(std::mem::size_of::<SizeClass>() == vn_types::cell::SIZE_CLASS_STRIDE);

#[inline(always)]
pub(super) fn next_free(r: u64) -> *mut u64 {
    (r as usize + vn_types::cell::HEADER_BYTES) as *mut u64
}

#[inline(always)]
pub(super) fn cells_span(cell: usize) -> u64 {
    (cells_per_block(cell) * cell) as u64
}
