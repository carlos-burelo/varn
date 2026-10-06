use std::alloc::{alloc_zeroed, Layout};
use std::ptr::NonNull;
use varn_types::cell::{cells_per_block, BLOCK_BYTES, CELL_ALIGN};
use varn_types::HeapRef;

pub(super) fn block_layout() -> Layout {
    Layout::from_size_align(BLOCK_BYTES, CELL_ALIGN).expect("block layout")
}

pub(super) fn large_layout(bytes: usize) -> Layout {
    Layout::from_size_align(bytes, CELL_ALIGN).expect("large cell layout")
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct AllocLane {
    pub(crate) free: u64,
    pub(crate) bump: u64,
    pub(crate) end: u64,
}

#[derive(Default)]
pub(crate) struct SizeClass {
    pub(super) blocks: Vec<NonNull<u8>>,
    pub(super) lane: AllocLane,
}

#[inline(always)]
fn next_free(r: u64) -> *mut u64 {
    (r as usize + varn_types::cell::HEADER_BYTES) as *mut u64
}

impl SizeClass {
    #[inline]
    pub(super) fn take(&mut self, cell: usize) -> HeapRef {
        let lane = &mut self.lane;
        if lane.free != 0 {
            let r = lane.free;
            lane.free = unsafe { *next_free(r) };
            return unsafe { HeapRef::from_addr_unchecked(r) };
        }
        if lane.bump + cell as u64 > lane.end {
            let block = unsafe { alloc_zeroed(block_layout()) };
            let block = NonNull::new(block)
                .unwrap_or_else(|| std::alloc::handle_alloc_error(block_layout()));
            self.blocks.push(block);
            let base = block.as_ptr() as u64;
            lane.bump = base;
            lane.end = base + (cells_per_block(cell) * cell) as u64;
        }
        let r = lane.bump;
        lane.bump += cell as u64;
        unsafe { HeapRef::from_addr_unchecked(r) }
    }

    #[inline]
    pub(super) fn give_back(&mut self, r: HeapRef) {
        unsafe { *next_free(r.addr()) = self.lane.free };
        self.lane.free = r.addr();
    }

    pub(super) fn free_len(&self) -> usize {
        let mut n = 0;
        let mut r = self.lane.free;
        while r != 0 {
            n += 1;
            r = unsafe { *next_free(r) };
        }
        n
    }

    pub(super) fn cells(&self, cell: usize) -> impl Iterator<Item = HeapRef> + '_ {
        let last = self.blocks.len().saturating_sub(1);
        self.blocks.iter().enumerate().flat_map(move |(bi, block)| {
            let base = block.as_ptr() as usize;
            let used = if bi == last {
                (self.lane.bump as usize - base) / cell
            } else {
                cells_per_block(cell)
            };
            (0..used)
                .map(move |i| unsafe { HeapRef::from_addr_unchecked((base + i * cell) as u64) })
        })
    }
}
