//! Heap memory: aligned blocks of fixed-size cells. A reference is a cell's
//! address, so reaching an object is a load from that address; each block
//! keeps the generation state of its cells in its header, reached from any
//! cell address by masking.

use super::obj::HeapObj;
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr::NonNull;
use varn_types::HeapRef;

pub(crate) const BLOCK_BYTES: usize = 256 * 1024;
pub(crate) const CELL_BYTES: usize = std::mem::size_of::<HeapObj>();
const MAX_CELLS: usize = BLOCK_BYTES / (CELL_BYTES + 1);
pub(crate) const CELLS_OFFSET: usize = MAX_CELLS.div_ceil(64) * 64;
pub(crate) const CELLS_PER_BLOCK: usize = (BLOCK_BYTES - CELLS_OFFSET) / CELL_BYTES;
const MAJOR_MARK: u8 = 0x80;

const _: () = assert!(CELLS_PER_BLOCK <= CELLS_OFFSET);
const _: () = assert!(CELLS_OFFSET.is_multiple_of(std::mem::align_of::<HeapObj>()));

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SlotState {
    Free = 0,
    Old = 1,
    Remembered = 2,
    Young = 3,
    Marked = 4,
}

impl SlotState {
    #[inline(always)]
    fn from_byte(b: u8) -> Self {
        match b & !MAJOR_MARK {
            1 => Self::Old,
            2 => Self::Remembered,
            3 => Self::Young,
            4 => Self::Marked,
            _ => Self::Free,
        }
    }
}

#[inline(always)]
fn state_byte(r: HeapRef) -> *mut u8 {
    let addr = r.addr() as usize;
    let block = addr & !(BLOCK_BYTES - 1);
    (block + (addr - block - CELLS_OFFSET) / CELL_BYTES) as *mut u8
}

fn block_layout() -> Layout {
    Layout::from_size_align(BLOCK_BYTES, BLOCK_BYTES).expect("block layout")
}

pub(crate) struct CellSpace {
    blocks: Vec<NonNull<u8>>,
    bump: usize,
    free: Vec<HeapRef>,
    live: usize,
    pub(crate) births: u64,
    pub(crate) old_growth: u64,
}

impl Default for CellSpace {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            bump: CELLS_PER_BLOCK,
            free: Vec::new(),
            live: 0,
            births: 0,
            old_growth: 0,
        }
    }
}

impl CellSpace {
    #[inline]
    pub(crate) fn alloc(&mut self, obj: HeapObj, state: SlotState) -> HeapRef {
        self.births += 1;
        if state == SlotState::Old {
            self.old_growth += 1;
        }
        self.live += 1;
        let r = match self.free.pop() {
            Some(r) => r,
            None => self.bump_cell(),
        };
        unsafe {
            std::ptr::write(r.as_ptr::<HeapObj>(), obj);
            *state_byte(r) = state as u8;
        }
        r
    }

    #[cold]
    fn new_block(&mut self) {
        let block = unsafe { alloc_zeroed(block_layout()) };
        let block =
            NonNull::new(block).unwrap_or_else(|| std::alloc::handle_alloc_error(block_layout()));
        self.blocks.push(block);
        self.bump = 0;
    }

    #[inline]
    fn bump_cell(&mut self) -> HeapRef {
        if self.bump == CELLS_PER_BLOCK {
            self.new_block();
        }
        let block = self
            .blocks
            .last()
            .expect("a block after new_block")
            .as_ptr() as usize;
        let addr = block + CELLS_OFFSET + self.bump * CELL_BYTES;
        self.bump += 1;
        unsafe { HeapRef::from_addr_unchecked(addr as u64) }
    }

    /// The object `r` names. `r` must come from this space and still be live.
    #[inline(always)]
    pub(crate) fn get(&self, r: HeapRef) -> &HeapObj {
        debug_assert_ne!(self.state(r), SlotState::Free, "reference to a freed cell");
        unsafe { &*r.as_ptr::<HeapObj>() }
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, r: HeapRef) -> &mut HeapObj {
        debug_assert_ne!(self.state(r), SlotState::Free, "reference to a freed cell");
        unsafe { &mut *r.as_ptr::<HeapObj>() }
    }

    #[inline(always)]
    pub(crate) fn state(&self, r: HeapRef) -> SlotState {
        SlotState::from_byte(unsafe { *state_byte(r) })
    }

    #[inline(always)]
    pub(crate) fn set_state(&mut self, r: HeapRef, state: SlotState) {
        unsafe { *state_byte(r) = state as u8 };
    }

    pub(crate) fn promote(&mut self, r: HeapRef) {
        self.set_state(r, SlotState::Old);
        self.old_growth += 1;
    }

    /// Drops the object where it stands and frees its cell.
    #[inline(always)]
    pub(crate) fn release(&mut self, r: HeapRef) {
        unsafe {
            std::ptr::drop_in_place(r.as_ptr::<HeapObj>());
            *state_byte(r) = SlotState::Free as u8;
        }
        self.free.push(r);
        self.live -= 1;
    }

    /// Marks a young object reached by a minor collection, queueing it once.
    #[inline(always)]
    pub(crate) fn mark_young(r: HeapRef, work: &mut Vec<HeapRef>) {
        unsafe {
            let b = state_byte(r);
            if *b == SlotState::Young as u8 {
                *b = SlotState::Marked as u8;
                work.push(r);
            }
        }
    }

    /// Sets the major-collection mark, answering whether it was clear.
    #[inline(always)]
    pub(crate) fn mark_major(r: HeapRef) -> bool {
        unsafe {
            let b = state_byte(r);
            if *b == SlotState::Free as u8 || *b & MAJOR_MARK != 0 {
                return false;
            }
            *b |= MAJOR_MARK;
            true
        }
    }

    pub(crate) fn live_count(&self) -> usize {
        self.live
    }

    pub(crate) fn free_len(&self) -> usize {
        self.free.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.blocks.len() * CELLS_PER_BLOCK
    }

    fn used_in(&self, block_idx: usize) -> usize {
        if block_idx + 1 == self.blocks.len() {
            self.bump
        } else {
            CELLS_PER_BLOCK
        }
    }

    /// Every live cell, in address order within each block.
    pub(crate) fn refs(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.blocks.iter().enumerate().flat_map(move |(bi, block)| {
            let base = block.as_ptr() as usize;
            (0..self.used_in(bi)).filter_map(move |i| {
                let live = unsafe { *(base as *const u8).add(i) } != SlotState::Free as u8;
                live.then(|| unsafe {
                    HeapRef::from_addr_unchecked((base + CELLS_OFFSET + i * CELL_BYTES) as u64)
                })
            })
        })
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (HeapRef, &HeapObj, SlotState)> + '_ {
        self.refs().map(move |r| (r, self.get(r), self.state(r)))
    }

    /// Frees every live cell without the major mark, clears the mark on the
    /// rest and leaves them old. Returns how many were freed.
    pub(crate) fn sweep_major(&mut self) -> usize {
        let mut dead: Vec<HeapRef> = Vec::new();
        for r in self.refs() {
            let b = state_byte(r);
            unsafe {
                if *b & MAJOR_MARK != 0 {
                    *b = SlotState::Old as u8;
                } else {
                    dead.push(r);
                }
            }
        }
        for &r in &dead {
            self.release(r);
        }
        dead.len()
    }
}

impl Drop for CellSpace {
    fn drop(&mut self) {
        let live: Vec<HeapRef> = self.refs().collect();
        for r in live {
            unsafe { std::ptr::drop_in_place(r.as_ptr::<HeapObj>()) };
        }
        for block in &self.blocks {
            unsafe { dealloc(block.as_ptr(), block_layout()) };
        }
    }
}
