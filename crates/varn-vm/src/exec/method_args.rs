//! Where a method call's arguments are, and what the call left behind.
//!
//! One method-call implementation serves the interpreter, whose arguments are
//! a window of registers of the caller's activation, and compiled code from
//! typed SSA, whose argument values live in arbitrary homes and arrive as a
//! boxed list. [`MethodArgs`] is the only thing that differs between them.

use crate::error::VmResult;
use crate::frame_store::FrameStore;
use crate::value::VmValue;

#[derive(Clone, Copy)]
pub(crate) enum MethodArgs<'a> {
    /// `count` registers from `start` of activation `base`.
    Regs {
        base: usize,
        start: usize,
        count: usize,
    },
    /// Boxed values, in order.
    Boxed(&'a [VmValue]),
}

impl MethodArgs<'_> {
    pub(crate) fn len(&self) -> usize {
        match self {
            MethodArgs::Regs { count, .. } => *count,
            MethodArgs::Boxed(values) => values.len(),
        }
    }

    /// Argument `i`, boxed.
    pub(crate) fn get(&self, stack: &FrameStore, i: usize) -> VmValue {
        match self {
            MethodArgs::Regs { base, start, .. } => stack.box_reg(*base, start + i),
            MethodArgs::Boxed(values) => values[i],
        }
    }

    /// Write the first `n` arguments into registers `dst..` of activation
    /// `alloc`, converted to each register's class.
    pub(crate) fn copy_into(
        &self,
        stack: &mut FrameStore,
        alloc: usize,
        dst: usize,
        n: usize,
    ) -> VmResult<()> {
        for i in 0..n {
            match self {
                MethodArgs::Regs { base, start, .. } => {
                    stack.mov_cross(alloc, dst + i, *base, start + i)?
                }
                MethodArgs::Boxed(values) => stack.unbox_into_reg(alloc, dst + i, values[i])?,
            }
        }
        Ok(())
    }
}

/// What a method call left: its value, or a pushed activation of a VM
/// method that has yet to run (the interpreter continues into it; compiled
/// code runs it to completion).
pub(crate) enum MethodOutcome {
    Value(VmValue),
    FramePushed,
}
