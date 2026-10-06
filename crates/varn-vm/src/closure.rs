use crate::frame_store::{FrameStore, SlotAddr};
use crate::value::VmValue;
use std::cell::RefCell;
use std::rc::Rc;
use varn_types::chunk::PolyICSlot;
use varn_types::FunctionProto;

#[derive(Debug, Clone)]
pub struct VmUpvalue {
    pub inner: Rc<RefCell<VmUpvalueInner>>,
}

#[derive(Debug, Clone)]
pub struct VmUpvalueInner {
    pub value: VmValue,
    pub stack_slot: Option<SlotAddr>,
}

impl VmUpvalue {
    pub(crate) fn open(stack_slot: SlotAddr) -> Self {
        Self {
            inner: Rc::new(RefCell::new(VmUpvalueInner {
                value: VmValue::null(),
                stack_slot: Some(stack_slot),
            })),
        }
    }

    pub(crate) fn closed(value: VmValue) -> Self {
        Self {
            inner: Rc::new(RefCell::new(VmUpvalueInner {
                value,
                stack_slot: None,
            })),
        }
    }

    pub(crate) fn read(&self, store: &FrameStore) -> VmValue {
        let g = self.inner.borrow_mut();
        match g.stack_slot {
            Some(slot) => store.get_addr(slot),
            None => g.value,
        }
    }

    pub(crate) fn write(&self, val: VmValue, store: &mut FrameStore) -> crate::error::VmResult<()> {
        let mut g = self.inner.borrow_mut();
        match g.stack_slot {
            Some(slot) => store.set_addr(slot, val),
            None => {
                g.value = val;
                Ok(())
            }
        }
    }

    pub(crate) fn close(&self, store: &FrameStore) {
        let mut g = self.inner.borrow_mut();
        if let Some(slot) = g.stack_slot.take() {
            g.value = store.get_addr(slot);
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VmClosure {
    pub proto: Rc<FunctionProto>,
    pub upvalues: Vec<VmUpvalue>,
    pub constants: Rc<Vec<VmValue>>,
    pub ic_cache: Rc<RefCell<Vec<PolyICSlot>>>,
    pub feedback: Rc<RefCell<varn_types::chunk::FeedbackVector>>,

    pub ic_entries: *const PolyICSlot,

    pub module_base: u32,
}

impl VmClosure {
    pub(crate) fn new(
        proto: Rc<FunctionProto>,
        constants: Vec<VmValue>,
        settings: crate::settings::ExecSettings,
    ) -> Self {
        proto.ensure_ic();
        let ic_cache = Rc::clone(&proto.ic_cache);
        let feedback = Rc::clone(&proto.feedback);
        let ic_entries = unsafe { (*ic_cache.as_ptr()).as_ptr() };
        let closure = Self {
            proto,
            upvalues: Vec::new(),
            constants: Rc::new(constants),
            ic_cache,
            feedback,
            ic_entries,
            module_base: 0,
        };

        let _ = settings;
        closure
    }

    pub(crate) fn with_upvalues(
        proto: Rc<FunctionProto>,
        upvalues: Vec<VmUpvalue>,
        constants: Rc<Vec<VmValue>>,
        settings: crate::settings::ExecSettings,
    ) -> Self {
        proto.ensure_ic();
        let ic_cache = Rc::clone(&proto.ic_cache);
        let feedback = Rc::clone(&proto.feedback);
        let ic_entries = unsafe { (*ic_cache.as_ptr()).as_ptr() };
        let closure = Self {
            proto,
            upvalues,
            constants,
            ic_cache,
            feedback,
            ic_entries,
            module_base: 0,
        };

        let _ = settings;
        closure
    }
    #[inline(always)]
    pub(crate) fn ic_cache_len(&self) -> usize {
        self.ic_cache.borrow().len()
    }
}
