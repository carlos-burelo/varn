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
    pub return_reg: Option<u16>,
    pub caller_base: Option<usize>,
}

unsafe impl Send for CallFrame {}
unsafe impl Sync for CallFrame {}

impl CallFrame {
    pub(crate) fn new(closure: &VmClosure, base: usize) -> Self {
        Self {
            closure_ptr: closure as *const VmClosure,
            _owned_closure: None,
            ip: 0,
            base,
            current_class: None,
            return_reg: None,
            caller_base: None,
        }
    }

    pub(crate) fn new_owned(closure: Rc<VmClosure>, base: usize) -> Self {
        Self {
            closure_ptr: Rc::as_ptr(&closure),
            _owned_closure: Some(closure),
            ip: 0,
            base,
            current_class: None,
            return_reg: None,
            caller_base: None,
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
