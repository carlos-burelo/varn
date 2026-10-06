





use crate::closure::VmClosure;
use std::rc::Rc;

pub const MAX_CALL_DEPTH: usize = 10000;

pub const VM_STACK_BYTES: usize = 64 << 20;

#[repr(C)]
pub struct CallFrame {
    pub closure_ptr: *const VmClosure,
    pub _owned_closure: Option<Rc<VmClosure>>,
    pub ip: usize,
    pub base: usize,
    pub current_class: Option<Rc<varn_types::ClassObj>>,
    
    
    
    
    
    
    
    
    
    
    pub return_reg: u16,
}

unsafe impl Send for CallFrame {}
unsafe impl Sync for CallFrame {}

impl CallFrame {
    
    
    
    
    
    pub const NO_RETURN_REG: u16 = u16::MAX;

    
    
    pub const NO_ACTIVATION: usize = usize::MAX;

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
