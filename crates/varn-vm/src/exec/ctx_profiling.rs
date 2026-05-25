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
    pub fn record_reg_load(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_reg_load();
        }
    }

    #[inline(always)]
    pub fn record_reg_store(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_reg_store();
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
}
