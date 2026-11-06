use std::sync::atomic::{AtomicUsize, Ordering};

use crate::frame::CallFrame;
use crate::frame_stack::FrameStack;

pub(super) const MAX_DEPTH: usize = 192;

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct Entry {
    pub proto: usize,
    pub ip: usize,
}

#[repr(C)]
pub(super) struct Slot {
    pub depth: u32,
    pub truncated: bool,
    pub pc: usize,
    pub frames: [Entry; MAX_DEPTH],
}

pub(crate) struct Shared {
    slots: *mut Slot,
    capacity: usize,
    taken: AtomicUsize,
    frames: *const FrameStack,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Shared {
    pub(super) fn new(capacity: usize, frames: *const FrameStack) -> Self {
        let slots = Box::<[Slot]>::new_zeroed_slice(capacity);
        let slots = Box::into_raw(unsafe { slots.assume_init() }) as *mut Slot;
        Self {
            slots,
            capacity,
            taken: AtomicUsize::new(0),
            frames,
        }
    }

    pub(super) fn recorded(&self) -> usize {
        self.taken.load(Ordering::Acquire).min(self.capacity)
    }

    pub(super) fn dropped(&self) -> usize {
        self.taken
            .load(Ordering::Acquire)
            .saturating_sub(self.capacity)
    }

    pub(super) fn slot(&self, index: usize) -> &Slot {
        assert!(index < self.recorded());
        unsafe { &*self.slots.add(index) }
    }

    pub(crate) unsafe fn capture(&self, pc: usize) {
        let index = self.taken.fetch_add(1, Ordering::AcqRel);
        if index >= self.capacity {
            return;
        }
        let slot = unsafe { &mut *self.slots.add(index) };
        let stack: &[CallFrame] = unsafe { &*self.frames };
        let skipped = stack.len().saturating_sub(MAX_DEPTH);
        slot.truncated = skipped > 0;
        slot.pc = pc;
        let mut depth = 0;
        for frame in &stack[skipped..] {
            slot.frames[depth] = Entry {
                proto: unsafe { std::rc::Rc::as_ptr(&(*frame.closure_ptr).proto) } as usize,
                ip: frame.ip,
            };
            depth += 1;
        }
        slot.depth = depth as u32;
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        drop(unsafe {
            Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.slots,
                self.capacity,
            ))
        });
    }
}
