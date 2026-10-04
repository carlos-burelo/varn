use std::alloc::{alloc_zeroed, Layout};
use std::ptr::NonNull;
use varn_types::HeapRef;

pub(super) const BLOCK_BYTES: usize = 256 * 1024;
pub(super) const CELL_ALIGN: usize = 16;
pub(super) const CLASS_BYTES: [usize; 24] = [
    16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768,
    1024, 1536, 2048,
];

pub(super) fn class_for(bytes: usize) -> Option<usize> {
    CLASS_BYTES.iter().position(|&c| c >= bytes)
}

pub(super) fn block_layout() -> Layout {
    Layout::from_size_align(BLOCK_BYTES, CELL_ALIGN).expect("block layout")
}

pub(super) fn large_layout(bytes: usize) -> Layout {
    Layout::from_size_align(bytes, CELL_ALIGN).expect("large cell layout")
}

#[derive(Default)]
pub(super) struct SizeClass {
    pub(super) blocks: Vec<NonNull<u8>>,
    used_in_last: usize,
    pub(super) free: Vec<HeapRef>,
}

impl SizeClass {
    pub(super) fn cells_per_block(cell: usize) -> usize {
        BLOCK_BYTES / cell
    }

    pub(super) fn take(&mut self, cell: usize) -> HeapRef {
        if let Some(r) = self.free.pop() {
            return r;
        }
        if self.blocks.is_empty() || self.used_in_last == Self::cells_per_block(cell) {
            let block = unsafe { alloc_zeroed(block_layout()) };
            let block = NonNull::new(block)
                .unwrap_or_else(|| std::alloc::handle_alloc_error(block_layout()));
            self.blocks.push(block);
            self.used_in_last = 0;
        }
        let base = self.blocks.last().expect("a block").as_ptr() as usize;
        let addr = base + self.used_in_last * cell;
        self.used_in_last += 1;
        unsafe { HeapRef::from_addr_unchecked(addr as u64) }
    }

    pub(super) fn cells(&self, cell: usize) -> impl Iterator<Item = HeapRef> + '_ {
        let last = self.blocks.len().saturating_sub(1);
        self.blocks.iter().enumerate().flat_map(move |(bi, block)| {
            let used = if bi == last {
                self.used_in_last
            } else {
                Self::cells_per_block(cell)
            };
            let base = block.as_ptr() as usize;
            (0..used)
                .map(move |i| unsafe { HeapRef::from_addr_unchecked((base + i * cell) as u64) })
        })
    }
}
