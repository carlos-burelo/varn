//! The closure value model: captured variables and the runtime closure the
//! interpreter and compiled code both execute.
//!
//! A `VmClosure` binds a `FunctionProto` (shared, immutable code) to one
//! activation's captured state. The JIT tiering that decides whether that
//! proto gets compiled lives in [`crate::jit::tiering`] — it is policy about
//! code, not part of the value.

use crate::value::VmValue;
use std::cell::RefCell;
use std::rc::Rc;
use varn_core::VmValuePayload;
use varn_types::chunk::PolyICSlot;
use varn_types::FunctionProto;
pub use varn_types::VmValueRef;

#[derive(Debug, Clone)]
pub struct VmUpvalue {
    pub inner: Rc<RefCell<VmUpvalueInner>>,
}

#[derive(Debug, Clone)]
pub struct VmUpvalueInner {
    pub value: VmValue,
    pub stack_slot: Option<usize>,
}

impl VmUpvalue {
    pub(crate) fn open(stack_slot: usize) -> Self {
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

    pub(crate) fn read(&self, stack: &[VmValue]) -> VmValue {
        let g = self.inner.borrow_mut();
        match g.stack_slot {
            Some(slot) => stack[slot],
            None => g.value,
        }
    }

    pub(crate) fn write(&self, val: VmValue, stack: &mut [VmValue]) {
        let mut g = self.inner.borrow_mut();
        match g.stack_slot {
            Some(slot) => stack[slot] = val,
            None => g.value = val,
        }
    }

    pub(crate) fn close(&self, stack: &[VmValue]) {
        let mut g = self.inner.borrow_mut();
        if let Some(slot) = g.stack_slot.take() {
            g.value = stack[slot];
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
        let closure = Self {
            proto,
            upvalues: Vec::new(),
            constants: Rc::new(constants),
            ic_cache,
            feedback,
        };
        // No compilation here: the compiled entry lives on the proto and is
        // produced lazily by `hot_jit_fn` once the function proves hot (see
        // `FunctionProto::jit_entry_count`). `settings.no_jit` is honoured at
        // that point, so a run meant to isolate a codegen bug never invokes
        // codegen.
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
        let closure = Self {
            proto,
            upvalues,
            constants,
            ic_cache,
            feedback,
        };
        // No compilation here: the compiled entry lives on the proto and is
        // produced lazily by `hot_jit_fn` once the function proves hot (see
        // `FunctionProto::jit_entry_count`). `settings.no_jit` is honoured at
        // that point, so a run meant to isolate a codegen bug never invokes
        // codegen.
        let _ = settings;
        closure
    }
    #[inline(always)]
    pub(crate) fn ic_cache_len(&self) -> usize {
        self.ic_cache.borrow().len()
    }
}

#[derive(Debug, Clone)]
pub struct VmClosurePayload(pub Rc<VmClosure>);

impl VmValuePayload for VmClosurePayload {
    fn clone_payload(&self) -> Box<dyn VmValuePayload> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl VmClosurePayload {
    #[inline(always)]
    pub fn downcast_from(payload: &dyn VmValuePayload) -> Option<&Rc<VmClosure>> {
        payload
            .as_any()
            .downcast_ref::<VmClosurePayload>()
            .map(|w| &w.0)
    }
}
