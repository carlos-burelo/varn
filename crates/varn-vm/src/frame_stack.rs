


















use crate::frame::CallFrame;
use crate::profile::ProfileCounters;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct FrameStack {
    frames: Vec<CallFrame>,
    
    counters: Option<Arc<ProfileCounters>>,
}

impl FrameStack {
    pub(crate) fn frames_field_offset() -> usize {
        std::mem::offset_of!(FrameStack, frames)
    }

    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            frames: Vec::with_capacity(cap),
            counters: None,
        }
    }

    
    
    
    pub(crate) fn set_counters(&mut self, counters: Option<Arc<ProfileCounters>>) {
        self.counters = counters;
    }

    #[inline(always)]
    pub fn push(&mut self, frame: CallFrame) {
        if let Some(ref c) = self.counters {
            c.frame_pushes.fetch_add(1, Ordering::Relaxed);
        }
        self.frames.push(frame);
    }

    #[inline(always)]
    pub fn pop(&mut self) -> Option<CallFrame> {
        let popped = self.frames.pop();
        if popped.is_some() {
            if let Some(ref c) = self.counters {
                c.frame_pops.fetch_add(1, Ordering::Relaxed);
            }
        }
        popped
    }
}

impl Deref for FrameStack {
    type Target = Vec<CallFrame>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.frames
    }
}

impl DerefMut for FrameStack {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.frames
    }
}



impl<'a> IntoIterator for &'a FrameStack {
    type Item = &'a CallFrame;
    type IntoIter = std::slice::Iter<'a, CallFrame>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.frames.iter()
    }
}

impl<'a> IntoIterator for &'a mut FrameStack {
    type Item = &'a mut CallFrame;
    type IntoIter = std::slice::IterMut<'a, CallFrame>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.frames.iter_mut()
    }
}
