//! Heap memory: every object is a cell that starts with an 8-byte header —
//! generation state, kind, size class — followed by its body. A reference is
//! the cell's address. Cells come from blocks segregated by size class, or
//! from their own allocation when larger than the largest class.

use super::obj::HeapObj;
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::ptr::NonNull;
use std::rc::Rc;
use varn_types::value::{InstanceData, InstanceRef, ObjData, Shape};
use varn_types::HeapRef;

pub(crate) const HEADER_BYTES: usize = std::mem::size_of::<ObjHeader>();
pub(crate) const STATE_OFF: usize = std::mem::offset_of!(ObjHeader, state);
pub(crate) const KIND_OFF: usize = std::mem::offset_of!(ObjHeader, kind);
pub(crate) const INSTANCE_DATA_OFF: usize = HEADER_BYTES + std::mem::size_of::<HeapObj>();
const BLOCK_BYTES: usize = 256 * 1024;
const CELL_ALIGN: usize = 16;
const MAJOR_MARK: u8 = 0x80;
const LARGE: u8 = u8::MAX;
const CLASS_BYTES: [usize; 24] = [
    16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768,
    1024, 1536, 2048,
];

const _: () = assert!(HEADER_BYTES == 8);
const _: () = assert!(HEADER_BYTES.is_multiple_of(std::mem::align_of::<HeapObj>()));

#[repr(C)]
struct ObjHeader {
    state: u8,
    kind: u8,
    class: u8,
    _pad: u8,
    _aux: u32,
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
fn header<'a>(r: HeapRef) -> &'a mut ObjHeader {
    unsafe { &mut *r.as_ptr::<ObjHeader>() }
}

#[inline(always)]
fn body<T>(r: HeapRef) -> *mut T {
    (r.addr() as usize + HEADER_BYTES) as *mut T
}

fn class_for(bytes: usize) -> Option<usize> {
    CLASS_BYTES.iter().position(|&c| c >= bytes)
}

fn block_layout() -> Layout {
    Layout::from_size_align(BLOCK_BYTES, CELL_ALIGN).expect("block layout")
}

fn large_layout(bytes: usize) -> Layout {
    Layout::from_size_align(bytes, CELL_ALIGN).expect("large cell layout")
}

#[derive(Default)]
struct SizeClass {
    blocks: Vec<NonNull<u8>>,
    used_in_last: usize,
    free: Vec<HeapRef>,
}

impl SizeClass {
    fn cells_per_block(cell: usize) -> usize {
        BLOCK_BYTES / cell
    }

    fn take(&mut self, cell: usize) -> HeapRef {
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

    fn cells(&self, cell: usize) -> impl Iterator<Item = HeapRef> + '_ {
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

/// Where native code was when it entered: restored when it returns.
pub(crate) struct NativeScope {
    was_native: bool,
    roots_len: usize,
}

pub(crate) struct CellSpace {
    classes: Vec<SizeClass>,
    large: Vec<(HeapRef, usize)>,
    live: usize,
    native: bool,
    native_roots: Vec<HeapRef>,
    pub(crate) births: u64,
    pub(crate) old_growth: u64,
}

impl Default for CellSpace {
    fn default() -> Self {
        Self {
            classes: CLASS_BYTES.iter().map(|_| SizeClass::default()).collect(),
            large: Vec::new(),
            live: 0,
            native: false,
            native_roots: Vec::new(),
            births: 0,
            old_growth: 0,
        }
    }
}

impl CellSpace {
    /// A cell of at least `body_bytes` after the header, its header written
    /// and its body left for the caller to fill.
    #[inline]
    fn take_cell(&mut self, body_bytes: usize, kind: u8, state: SlotState) -> HeapRef {
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
        self.births += 1;
        if state == SlotState::Old {
            self.old_growth += 1;
        }
        self.live += 1;
        if self.native {
            self.native_roots.push(r);
        }
        r
    }

    #[inline]
    pub(crate) fn alloc(&mut self, obj: HeapObj, state: SlotState) -> HeapRef {
        let r = self.take_cell(std::mem::size_of::<HeapObj>(), 0, state);
        Self::place(r, obj);
        r
    }

    /// An instance whose payload lives in the same cell, right after the
    /// `HeapObj` that names it: one allocation, no separate body.
    #[inline]
    pub(crate) fn alloc_instance(
        &mut self,
        class_id: u32,
        payload_size: u32,
        state: SlotState,
    ) -> (HeapRef, InstanceRef) {
        let tail = INSTANCE_DATA_OFF - HEADER_BYTES;
        let r = self.take_cell(tail + InstanceData::bytes_for(payload_size), 0, state);
        let data = (r.addr() as usize + INSTANCE_DATA_OFF) as *mut u8;
        let inst = unsafe { InstanceData::init_at(data, class_id, payload_size) };
        Self::place(r, HeapObj::Instance(inst));
        (r, inst)
    }

    /// A property object (`record` for a record) whose fields live in the
    /// same cell, right after the `HeapObj` that names it.
    #[inline]
    pub(crate) fn alloc_object(
        &mut self,
        record: bool,
        shape: Rc<Shape>,
        n: usize,
        values: &[varn_types::VmValue],
        state: SlotState,
    ) -> HeapRef {
        let tail = std::mem::size_of::<HeapObj>();
        let r = self.take_cell(tail + ObjData::bytes_for(n), 0, state);
        let obj = unsafe { ObjData::init_at(body::<u8>(r).add(tail), shape, n, values) };
        Self::place(
            r,
            if record {
                HeapObj::Record(obj)
            } else {
                HeapObj::Object(obj)
            },
        );
        r
    }

    /// An array (`tuple` for a tuple) whose repr lives in the same cell,
    /// right after the `HeapObj` that names it.
    #[inline]
    pub(crate) fn alloc_array(
        &mut self,
        tuple: bool,
        repr: varn_types::vm_value::ArrayRepr,
        state: SlotState,
    ) -> HeapRef {
        let tail = std::mem::size_of::<HeapObj>();
        let bytes = std::mem::size_of::<varn_types::vm_value::ArrayRepr>();
        let r = self.take_cell(tail + bytes, 0, state);
        let arr = unsafe { varn_types::VmArray::init_at(body::<u8>(r).add(tail).cast(), repr) };
        Self::place(
            r,
            if tuple {
                HeapObj::Tuple(arr)
            } else {
                HeapObj::Array(arr)
            },
        );
        r
    }

    /// Drops the object in `r` and what its cell owns beyond it.
    unsafe fn drop_object(r: HeapRef) {
        let obj = body::<HeapObj>(r);
        match &*obj {
            HeapObj::Object(o) | HeapObj::Record(o) => ObjData::drop_at(*o),
            HeapObj::Array(a) | HeapObj::Tuple(a) => a.drop_at(),
            _ => {}
        }
        std::ptr::drop_in_place(obj);
    }

    #[inline(always)]
    fn place(r: HeapRef, obj: HeapObj) {
        header(r).kind = unsafe { *(&obj as *const HeapObj as *const u8) };
        unsafe { std::ptr::write(body::<HeapObj>(r), obj) };
    }

    /// Native code may hold what it allocates only in Rust locals, across a
    /// callback that collects: from here until [`Self::exit_native`], every
    /// cell it takes is a root.
    pub(crate) fn enter_native(&mut self) -> NativeScope {
        let scope = NativeScope {
            was_native: self.native,
            roots_len: self.native_roots.len(),
        };
        self.native = true;
        scope
    }

    pub(crate) fn exit_native(&mut self, scope: NativeScope) {
        self.native = scope.was_native;
        self.native_roots.truncate(scope.roots_len);
    }

    /// Running VM code again, from inside native code or not: what it
    /// allocates is collectable as usual. Returns whether native code was
    /// running, for [`Self::resume`].
    pub(crate) fn suspend_native(&mut self) -> bool {
        std::mem::replace(&mut self.native, false)
    }

    /// Back from VM code. A value it handed to native code is a root for as
    /// long as that native code runs.
    pub(crate) fn resume(&mut self, was_native: bool, result: Option<HeapRef>) {
        self.native = was_native;
        if let (true, Some(r)) = (was_native, result) {
            self.native_roots.push(r);
        }
    }

    pub(crate) fn native_roots(&self) -> &[HeapRef] {
        &self.native_roots
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
        self.live -= 1;
        if class == LARGE {
            let at = self
                .large
                .iter()
                .position(|&(l, _)| l == r)
                .expect("a large cell");
            let (_, bytes) = self.large.swap_remove(at);
            unsafe { dealloc(r.as_ptr::<u8>(), large_layout(bytes)) };
        } else {
            self.classes[class as usize].free.push(r);
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
        self.live
    }

    pub(crate) fn free_len(&self) -> usize {
        self.classes.iter().map(|c| c.free.len()).sum()
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
        for r in self.refs() {
            let h = header(r);
            if h.state & MAJOR_MARK != 0 {
                h.state = SlotState::Old as u8;
            } else {
                dead.push(r);
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
