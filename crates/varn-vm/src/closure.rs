//! The closure value model: captured variables and the runtime closure the
//! interpreter and compiled code both execute.
//!
//! A `VmClosure` binds a `FunctionProto` (shared, immutable code) to one
//! activation's captured state. The JIT tiering that decides whether that
//! proto gets compiled lives in [`crate::jit::tiering`] — it is policy about
//! code, not part of the value.

use crate::frame_store::{FrameStore, SlotAddr};
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
    /// Raw data pointer of `ic_cache`'s `Vec<PolyICSlot>`, cached for the JIT to
    /// index a poly slot inline (`base + cs * POLY_IC_SLOT_SIZE`). The vec is
    /// fixed-size for a proto's life so this never dangles; the VM already
    /// reads the cache unsynchronised via `ic_cache.as_ptr()`.
    pub ic_entries: *const PolyICSlot,
    /// Start of this closure's module's global-slot region in the `GlobalStore`.
    /// `LoadGlobalIdx` / `StoreGlobalIdx` carry a slot relative to this. Set
    /// when the module is evaluated (top-level closure) and inherited by every
    /// nested closure at `MakeClosure`. `0` for the entry proto's own module
    /// and for protos with no module globals.
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
