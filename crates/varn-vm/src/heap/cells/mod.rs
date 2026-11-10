mod blocks;
mod native;
mod typed;

use super::obj::HeapObj;
use blocks::{block_layout, large_layout, SizeClass};
use std::alloc::{alloc_zeroed, dealloc};
use std::ptr::NonNull;
use varn_types::cell::{
    cells_per_block, class_for, CELL_CLASSES, CELL_CLASS_LARGE, HEADER_BYTES, HEADER_CLASS_OFF,
    HEADER_KIND_OFF, HEADER_STATE_OFF,
};
use varn_types::HeapRef;

const MAJOR_MARK: u8 = 0x80;

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
    pub(crate) classes: Vec<SizeClass>,
    large: Vec<(HeapRef, usize)>,
    pub(crate) native: bool,
    pub(in crate::heap) native_roots: Vec<HeapRef>,
    pub(crate) old_births: u64,
    pub(crate) survivors: usize,
    pub(crate) live_bytes: u64,
    pub(crate) abandon_objects: bool,
}

impl Default for CellSpace {
    fn default() -> Self {
        Self {
            classes: CELL_CLASSES.iter().map(|_| SizeClass::default()).collect(),
            large: Vec::new(),
            native: false,
            native_roots: Vec::new(),
            old_births: 0,
            survivors: 0,
            live_bytes: 0,
            abandon_objects: false,
        }
    }
}

impl CellSpace {
    #[inline]
    pub(in crate::heap) fn take_cell(
        &mut self,
        body_bytes: usize,
        kind: u8,
        state: SlotState,
    ) -> HeapRef {
        let bytes = HEADER_BYTES + body_bytes;
        assert!(
            bytes <= u32::MAX as usize,
            "a single cell over 4 GiB is not supported"
        );
        let (r, class) = match class_for(bytes) {
            Some(class) => (self.classes[class].take(CELL_CLASSES[class]), class as u8),
            None => {
                let ptr = unsafe { alloc_zeroed(large_layout(bytes)) };
                let ptr = NonNull::new(ptr)
                    .unwrap_or_else(|| std::alloc::handle_alloc_error(large_layout(bytes)));
                let r = unsafe { HeapRef::from_addr_unchecked(ptr.as_ptr() as u64) };
                self.large.push((r, bytes));
                (r, CELL_CLASS_LARGE)
            }
        };
        *header(r) = ObjHeader {
            state: state as u8,
            kind,
            class,
            _pad: 0,
            _aux: bytes as u32,
        };
        if state == SlotState::Old {
            self.old_births += 1;
        }
        if self.native {
            self.native_roots.push(r);
        }
        r
    }

    #[inline(always)]
    pub(crate) fn get(&self, r: HeapRef) -> &HeapObj {
        debug_assert_ne!(self.state(r), SlotState::Free, "reference to a freed cell");
        debug_assert_ne!(
            header(r).kind,
            varn_types::cell::CELL_KIND_INSTANCE,
            "colocated instance has no HeapObj; use instance()"
        );
        unsafe { &*body::<HeapObj>(r) }
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, r: HeapRef) -> &mut HeapObj {
        debug_assert_ne!(self.state(r), SlotState::Free, "reference to a freed cell");
        debug_assert_ne!(
            header(r).kind,
            varn_types::cell::CELL_KIND_INSTANCE,
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
        header(r).state = state as u8;
    }

    pub(crate) fn promote(&mut self, r: HeapRef) {
        self.set_state(r, SlotState::Old);
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

    pub(crate) fn release(&mut self, r: HeapRef) {
        let h = header(r);
        let class = h.class;
        if h.kind != varn_types::cell::CELL_KIND_INSTANCE {
            unsafe { Self::drop_object(r) };
        }
        h.state = SlotState::Free as u8;
        if class == CELL_CLASS_LARGE {
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

    #[inline(always)]
    pub(crate) fn mark_young(r: HeapRef, work: &mut Vec<HeapRef>) {
        let h = header(r);
        if h.state == SlotState::Young as u8 {
            h.state = SlotState::Marked as u8;
            work.push(r);
        }
    }

    #[inline(always)]
    pub(crate) fn mark_major(r: HeapRef) -> bool {
        let h = header(r);
        if h.state == SlotState::Free as u8 || h.state & MAJOR_MARK != 0 {
            return false;
        }
        h.state |= MAJOR_MARK;
        true
    }

    #[inline(always)]
    pub(crate) fn is_marked(r: HeapRef) -> bool {
        header(r).state & MAJOR_MARK != 0
    }

    pub(crate) fn used_cells(&self) -> usize {
        self.classes
            .iter()
            .zip(CELL_CLASSES)
            .map(|(c, cell)| c.used(cell))
            .sum::<usize>()
            + self.large.len()
    }

    pub(crate) fn free_list_problems(&self, problems: &mut Vec<String>) -> usize {
        let mut total = 0;
        for (class, cell) in self.classes.iter().zip(CELL_CLASSES) {
            let bound = class.blocks.len() * cells_per_block(cell);
            let mut addr = class.free_head();
            let mut seen = 0;
            while addr != 0 {
                if seen == bound {
                    problems.push(format!(
                        "free list of class {cell} loops after {bound} cells"
                    ));
                    break;
                }
                if !class.contains_addr(addr, cell) {
                    problems.push(format!(
                        "free list of class {cell} holds {addr:#x}, outside its blocks"
                    ));
                    break;
                }
                let h = header(unsafe { HeapRef::from_addr_unchecked(addr) });
                if h.state != SlotState::Free as u8 {
                    problems.push(format!(
                        "free list of class {cell} holds live cell {addr:#x} (state {})",
                        h.state
                    ));
                    break;
                }
                addr = blocks::SizeClass::free_next(addr);
                seen += 1;
            }
            total += seen;
        }
        total
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
            .zip(CELL_CLASSES)
            .map(|(c, cell)| c.blocks.len() * cells_per_block(cell))
            .sum::<usize>()
            + self.large.len()
    }

    pub(crate) fn refs(&self) -> impl Iterator<Item = HeapRef> + '_ {
        self.classes
            .iter()
            .zip(CELL_CLASSES)
            .flat_map(|(c, cell)| c.cells(cell))
            .chain(self.large.iter().map(|&(r, _)| r))
            .filter(|&r| header(r).state != SlotState::Free as u8)
    }

    pub(crate) fn sweep_major(&mut self) -> usize {
        let mut dead: Vec<HeapRef> = Vec::new();
        let mut survivors = 0;
        let mut live_bytes = 0u64;
        for r in self.refs() {
            let h = header(r);
            if h.state & MAJOR_MARK != 0 {
                h.state = SlotState::Old as u8;
                survivors += 1;
                live_bytes += if h.class == CELL_CLASS_LARGE {
                    h._aux as u64
                } else {
                    (unsafe { *CELL_CLASSES.get_unchecked(h.class as usize) }) as u64
                };
                if h.kind != varn_types::cell::CELL_KIND_INSTANCE {
                    live_bytes += crate::heap::bytes::external_bytes(self, r);
                }
            } else {
                dead.push(r);
            }
        }
        self.survivors = survivors;
        self.live_bytes = live_bytes;
        for &r in &dead {
            self.release(r);
        }
        dead.len()
    }
}

impl Drop for CellSpace {
    fn drop(&mut self) {
        if !self.abandon_objects {
            let live: Vec<HeapRef> = self.refs().collect();
            for r in live {
                if header(r).kind == varn_types::cell::CELL_KIND_INSTANCE {
                    continue;
                }
                unsafe { Self::drop_object(r) };
            }
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
