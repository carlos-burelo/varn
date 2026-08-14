//! The call-frame stack, and the only place frame pushes and pops are counted.
//!
//! The counters used to live on `ExecCtx` and be invoked by hand next to each
//! raw `frames.push` / `frames.pop`. They drifted: 10 of 14 push sites and 4 of
//! 8 pop sites carried one, so `bench -v` reported more pops than pushes
//! (31 882 vs 31 772) — a reading that cannot happen and that made the whole
//! frame section untrustworthy. Every JIT helper was among the silent sites.
//!
//! Putting the counter next to the `Vec` it counts makes the pairing structural
//! rather than remembered. `push` and `pop` are inherent methods, so they take
//! precedence over the ones reached through `Deref`, and every other `Vec`
//! operation (`len`, indexing, `last`, `iter`, `truncate`, …) passes through
//! untouched.
//!
//! Deliberately NOT an `ExecCtx` method: several call sites hold an immutable
//! borrow of `ctx.heap` across the push, and only a per-field borrow of
//! `ctx.frames` is legal there. A method on `ExecCtx` would borrow the whole
//! context and force an `Rc` clone onto the JIT's hot call path.

use crate::frame::CallFrame;
use crate::profile::ProfileCounters;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct FrameStack {
    frames: Vec<CallFrame>,
    /// `None` outside `bench -v`; the counting branch is then a null check.
    counters: Option<Arc<ProfileCounters>>,
}

impl FrameStack {
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            frames: Vec::with_capacity(cap),
            counters: None,
        }
    }

    /// Attach the profile counters. Called once, when the context is told to
    /// profile; frames pushed before that point are not counted, and neither is
    /// the run they belong to.
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

// `Deref` does not reach trait impls on `Vec`, and GC root scanning walks the
// frame stack with `for frame in &ctx.frames`.
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
