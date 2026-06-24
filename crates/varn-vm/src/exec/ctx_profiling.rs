use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;

use super::ctx::ExecCtx;

impl ExecCtx {
    pub fn push(&mut self, v: VmValue) {
        self.stack.push(v);
    }

    pub fn pop(&mut self) -> VmResult<VmValue> {
        self.stack
            .pop()
            .ok_or_else(|| RuntimeError::new("stack underflow"))
    }

    #[inline(always)]
    pub fn record_ic_hit(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit();
        }
    }

    #[inline(always)]
    pub fn record_ic_miss(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss();
        }
    }

    #[inline(always)]
    pub fn record_ic_hit_getprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_getprop();
        }
    }

    #[inline(always)]
    pub fn record_ic_miss_getprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_getprop();
        }
    }

    #[inline(always)]
    pub fn record_ic_hit_setprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_setprop();
        }
    }

    #[inline(always)]
    pub fn record_ic_miss_setprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_setprop();
        }
    }

    #[inline(always)]
    pub fn record_ic_hit_callmethod(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_callmethod();
        }
    }

    #[inline(always)]
    pub fn record_ic_miss_callmethod(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_callmethod();
        }
    }

    #[inline(always)]
    pub fn record_call_vm_fast(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_vm_fast();
        }
    }

    #[inline(always)]
    pub fn record_call_slow(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_slow();
        }
    }

    #[inline(always)]
    pub fn record_call_native(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_native();
        }
    }

    #[inline(always)]
    pub fn record_frame_push(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_frame_push();
        }
    }

    #[inline(always)]
    pub fn record_frame_pop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_frame_pop();
        }
    }

    #[inline(always)]
    pub fn record_hotspot_fn(&self, name: &str, jit: bool) {
        if let Some(ref h) = self.hotspot_counters {
            h.borrow_mut().record_fn_call(name, jit);
        }
    }

    #[inline(always)]
    pub fn record_hotspot_method(&self, name: &str, jit: bool) {
        if let Some(ref h) = self.hotspot_counters {
            h.borrow_mut().record_method_call(name, jit);
        }
    }

    #[inline(always)]
    pub fn record_hotspot_native(&self, name: &str) {
        if let Some(ref h) = self.hotspot_counters {
            h.borrow_mut().record_native_call(name);
        }
    }

    #[inline(always)]
    pub fn invoke_native(
        &mut self,
        f: varn_types::NativeFn,
        args: &[VmValue],
    ) -> Result<VmValue, String> {
        if self.hotspot_counters.is_none() {
            return (f)(self as &mut dyn varn_types::NativeCtx, args);
        }
        let start = std::time::Instant::now();
        let r = (f)(self as &mut dyn varn_types::NativeCtx, args);
        let ns = start.elapsed().as_nanos() as u64;
        if let Some(ref h) = self.hotspot_counters {
            h.borrow_mut().total_native_ns += ns;
        }
        r
    }

    #[inline(always)]
    pub fn record_hotspot_global(&self, idx: usize) {
        if let Some(ref h) = self.hotspot_counters {
            if let Some(name) = self.globals.idx_to_name.get(idx) {
                h.borrow_mut().record_global_access(name.clone());
            }
        }
    }

    #[inline(always)]
    pub fn record_hotspot_alloc(&self, type_name: &'static str) {
        if let Some(ref h) = self.hotspot_counters {
            h.borrow_mut().record_alloc(type_name);
        }
    }
}
