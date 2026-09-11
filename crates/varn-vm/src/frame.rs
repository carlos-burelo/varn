//! The interpreter call frame and its exception-handler record.
//!
//! A `CallFrame` is one activation: where its registers start in the shared
//! stack, where its instruction pointer is, and which closure it runs. It
//! deliberately holds a RAW pointer to that closure — see `closure_ptr`.

use crate::closure::VmClosure;
use std::rc::Rc;

#[repr(C)]
pub struct CallFrame {
    pub closure_ptr: *const VmClosure,
    pub _owned_closure: Option<Rc<VmClosure>>,
    pub ip: usize,
    pub base: usize,
    pub current_class: Option<Rc<varn_types::ClassObj>>,
    /// The caller-frame register this call's return value lands in, or
    /// [`Self::NO_RETURN_REG`]. Was `Option<u16>` — changed because `u16` has
    /// no spare bit pattern to niche the discriminant into, so `Option<u16>`'s
    /// in-memory shape is not something JIT-generated code can safely write
    /// without probing it out of the compiler first. Every field here except
    /// this one already niches to a null pointer for `None`/`0`, which is a
    /// guarantee this codebase already leans on elsewhere (see
    /// `Heap::rcbox_ptr_for_validation`'s `RcBox` layout assumption) — a
    /// sentinel `u16` gets the same "one representation, JIT can write it
    /// directly" property without adding a new one.
    pub return_reg: u16,
}

unsafe impl Send for CallFrame {}
unsafe impl Sync for CallFrame {}

impl CallFrame {
    /// Sentinel for "no return register" — this frame's result (if any) is
    /// not written back into a caller register (e.g. the top-level module
    /// frame, which has no caller). `u16::MAX` because register indices are
    /// encoded in an 8-bit bytecode operand field elsewhere in the pipeline,
    /// so no real register slot can ever reach it.
    pub const NO_RETURN_REG: u16 = u16::MAX;

    pub(crate) fn new(closure: &VmClosure, base: usize) -> Self {
        Self {
            closure_ptr: closure as *const VmClosure,
            _owned_closure: None,
            ip: 0,
            base,
            current_class: None,
            return_reg: Self::NO_RETURN_REG,
        }
    }

    pub(crate) fn new_owned(closure: Rc<VmClosure>, base: usize) -> Self {
        Self {
            closure_ptr: Rc::as_ptr(&closure),
            _owned_closure: Some(closure),
            ip: 0,
            base,
            current_class: None,
            return_reg: Self::NO_RETURN_REG,
        }
    }

    /// The closure this frame runs.
    ///
    /// Sound only because `closure_ptr` is kept alive for the frame's whole
    /// life: either by `_owned_closure` (frames built from an `Rc`) or by the
    /// caller's own live borrow (`new`).
    #[inline(always)]
    pub(crate) fn closure(&self) -> &VmClosure {
        unsafe { &*self.closure_ptr }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TryHandler {
    pub catch_ip: usize,
    pub frame_depth: usize,
    pub err_reg: u8,
}
