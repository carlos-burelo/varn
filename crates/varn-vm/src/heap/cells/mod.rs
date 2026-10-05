//! Heap memory: every object is a cell that starts with an 8-byte header —
//! generation state, kind, size class — followed by its body. A reference is
//! the cell's address. Cells come from blocks segregated by size class, or
//! from their own allocation when larger than the largest class.

mod blocks;
mod native;
mod typed;

use super::obj::HeapObj;
use blocks::{block_layout, class_for, large_layout, SizeClass, CLASS_BYTES};
use std::alloc::{alloc_zeroed, dealloc};
use std::ptr::NonNull;
use varn_types::HeapRef;

pub(crate) const HEADER_BYTES: usize = std::mem::size_of::<ObjHeader>();
pub(crate) const STATE_OFF: usize = std::mem::offset_of!(ObjHeader, state);
pub(crate) const KIND_OFF: usize = std::mem::offset_of!(ObjHeader, kind);
pub(crate) const INSTANCE_DATA_OFF: usize = HEADER_BYTES + std::mem::size_of::<HeapObj>();
const MAJOR_MARK: u8 = 0x80;
const LARGE: u8 = u8::MAX;

const _: () = assert!(HEADER_BYTES == 8);
const _: () = assert!(HEADER_BYTES.is_multiple_of(std::mem::align_of::<HeapObj>()));

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
pub(in crate::heap) fn header<'a>(r: HeapRef) -> &'a mut ObjHeader {
    unsafe { &mut *r.as_ptr::<ObjHeader>() }
}

#[inline(always)]
pub(in crate::heap) fn body<T>(r: HeapRef) -> *mut T {
    (r.addr() as usize + HEADER_BYTES) as *mut T
}

pub(crate) struct CellSpace {
    classes: Vec<SizeClass>,
    large: Vec<(HeapRef, usize)>,
    pub(in crate::heap) native: bool,
    pub(in crate::heap) native_roots: Vec<HeapRef>,
    pub(crate) old_births: u64,
    pub(crate) old_growth: u64,
    pub(crate) survivors: usize,
}

impl Default for CellSpace {
    fn default() -> Self {
        Self {
            classes: CLASS_BYTES.iter().map(|_| SizeClass::default()).collect(),
            large: Vec::new(),
            native: false,
            native_roots: Vec::new(),
            old_births: 0,
            old_growth: 0,
            survivors: 0,
        }
    }
}

impl CellSpace {
    /// A cell of at least `body_bytes` after the header, its header written
    /// and its body left for the caller to fill.
    #[inline]
    pub(in crate::heap) fn take_cell(
        &mut self,
        body_bytes: usize,
        kind: u8,
        state: SlotState,
    ) -> HeapRef {
        let bytes = HEADER_BYTES + body_bytes;
        let (r, class) = match class_for(bytes) {
            Some(class) => (self.classes[class].take(CLASS_BYTES[class]), class as u8),
            None => {
                let ptr = unsafe { alloc_zeroed(large_layout(bytes)) };
                let ptr = NonNull::new(ptr)
                    .unwrap_or_else(|| std::alloc::handle_alloc_error(large_layout(bytes)));
                let r = unsafe { HeapRef::from_addr_unchecked(ptr.as_ptr() as u64) };
                self.large.push((r, bytes));
                (r, LARGE)
            }
        };
        *header(r) = ObjHeader {
            state: state as u8,
            kind,
            class,
            _pad: 0,
            _aux: 0,
        };
        if state == SlotState::Old {
            self.old_births += 1;
            self.old_growth += 1;
        }
        if self.native {
            self.native_roots.push(r);
        }
        r
    }

    /// The object `r` names. `r` must come from this space and still be live.
    #[inline(always)]
    pub(crate) fn get(&self, r: HeapRef) -> &HeapObj {
        debug_assert_ne!(self.state(r), SlotState::Free, "reference to a freed cell");
        unsafe { &*body::<HeapObj>(r) }
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, r: HeapRef) -> &mut HeapObj {
        debug_assert_ne!(self.state(r), SlotState::Free, "reference to a freed cell");
        unsafe { &mut *body::<HeapObj>(r) }
    }

    #[inline(always)]
    pub(crate) fn state(&self, r: HeapRef) -> SlotState {
        SlotState::from_byte(header(r).state)
    }

    #[inline(always)]
    pub(crate) fn set_state(&mut self, r: HeapRef, state: SlotState) {
        header(r).state = state as u8;
    }

    pub(crate) fn promote(&mut self, r: HeapRef) {
        self.set_state(r, SlotState::Old);
        self.old_growth += 1;
    }

    /// Drops the object where it stands and frees its cell.
    pub(crate) fn release(&mut self, r: HeapRef) {
        let h = header(r);
        let class = h.class;
        unsafe { Self::drop_object(r) };
        h.state = SlotState::Free as u8;
        if class == LARGE {
            let at = self
                .large
                .iter()
                .position(|&(l, _)| l == r)
                .expect("a large cell");
            let (_, bytes) = self.large.swap_remove(at);
            unsafe { dealloc(r.as_ptr::<u8>(), large_layout(bytes)) };
        } else {
            self.classes[class as usize].give_back(r);
        }
    }

    /// Marks a young object reached by a minor collection, queueing it once.
    #[inline(always)]
    pub(crate) fn mark_young(r: HeapRef, work: &mut Vec<HeapRef>) {
        let h = header(r);
        if h.state == SlotState::Young as u8 {
            h.state = SlotState::Marked as u8;
            work.push(r);
        }
    }

    /// Sets the major-collection mark, answering whether it was clear.
    #[inline(always)]
    pub(crate) fn mark_major(r: HeapRef) -> bool {
        let h = header(r);
        if h.state == SlotState::Free as u8 || h.state & MAJOR_MARK != 0 {
            return false;
        }
        h.state |= MAJOR_MARK;
        true
    }

    pub(crate) fn live_count(&self) -> usize {
        self.refs().count()
    }

    pub(crate) fn free_len(&self) -> usize {
        self.classes.iter().map(SizeClass::free_len).sum()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.classes
            .iter()
            .zip(CLASS_BYTES)
            .map(|(c, cell)| c.blocks.len() * SizeClass::cells_per_block(cell))
            .sum::<usize>()
            + self.large.len()
    }

    /// Every live cell.
    pub(crate) fn refs(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.classes
            .iter()
            .zip(CLASS_BYTES)
            .flat_map(|(c, cell)| c.cells(cell))
            .chain(self.large.iter().map(|&(r, _)| r))
            .filter(|&r| header(r).state != SlotState::Free as u8)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (HeapRef, &HeapObj, SlotState)> + '_ {
        self.refs().map(move |r| (r, self.get(r), self.state(r)))
    }

    /// Frees every live cell without the major mark, clears the mark on the
    /// rest and leaves them old. Returns how many were freed.
    pub(crate) fn sweep_major(&mut self) -> usize {
        let mut dead: Vec<HeapRef> = Vec::new();
        let mut survivors = 0;
        for r in self.refs() {
            let h = header(r);
            if h.state & MAJOR_MARK != 0 {
                h.state = SlotState::Old as u8;
                survivors += 1;
            } else {
                dead.push(r);
            }
        }
        self.survivors = survivors;
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
            unsafe { Self::drop_object(r) };
        }
        for class in &self.classes {
            for block in &class.blocks {
                unsafe { dealloc(block.as_ptr(), block_layout()) };
            }
        }
        for &(r, bytes) in &self.large {
            unsafe { dealloc(r.as_ptr::<u8>(), large_layout(bytes)) };
        }
    }
}
