use std::sync::atomic::{AtomicUsize, Ordering};

use crate::frame::CallFrame;
use crate::frame_stack::FrameStack;

pub(super) const MAX_DEPTH: usize = 128;
pub(super) const NAME_BYTES: usize = 31;

#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct Entry {
    pub proto: usize,
    pub ip: usize,
    pub name_len: u8,
    pub name: [u8; NAME_BYTES],
}

impl Entry {
    pub(super) fn named(&self) -> &str {
        std::str::from_utf8(&self.name[..self.name_len as usize]).unwrap_or("")
    }
}

#[repr(C)]
pub(super) struct Slot {
    pub depth: u32,
    pub truncated: bool,
    pub activity: u8,
    pub pc: usize,
    pub cpu: u64,
    pub at_ns: u64,
    pub frames: [Entry; MAX_DEPTH],
}

pub(crate) struct Shared {
    slots: *mut Slot,
    capacity: usize,
    taken: AtomicUsize,
    frames: *const FrameStack,
    start: std::time::Instant,
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
            start: std::time::Instant::now(),
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

    pub(crate) unsafe fn capture(&self, pc: usize, cpu: u64) {
        let index = self.taken.fetch_add(1, Ordering::AcqRel);
        if index >= self.capacity {
            return;
        }
        let slot = unsafe { &mut *self.slots.add(index) };
        let stack: &[CallFrame] = unsafe { &*self.frames };
        let skipped = stack.len().saturating_sub(MAX_DEPTH);
        slot.truncated = skipped > 0;
        slot.pc = pc;
        slot.cpu = cpu;
        slot.at_ns = self.start.elapsed().as_nanos() as u64;
        slot.activity = super::activity::snapshot();
        let mut depth = 0;
        for frame in &stack[skipped..] {
            let proto = unsafe { &*std::rc::Rc::as_ptr(&(*frame.closure_ptr).proto) };
            let entry = &mut slot.frames[depth];
            entry.proto = proto as *const _ as usize;
            entry.ip = frame.ip;
            let name = proto.name.as_deref().unwrap_or("").as_bytes();
            let mut length = name.len().min(NAME_BYTES);
            while length > 0 && !std::str::from_utf8(&name[..length]).is_ok() {
                length -= 1;
            }
            entry.name_len = length as u8;
            entry.name[..length].copy_from_slice(&name[..length]);
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
