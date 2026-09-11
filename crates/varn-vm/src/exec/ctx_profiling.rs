use crate::value::VmValue;

use super::ctx::ExecCtx;

impl ExecCtx {
    pub(crate) fn push(&mut self, v: VmValue) {
        self.stack.push(v);
    }

    #[inline(always)]
    pub(crate) fn record_ic_hit_getprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_getprop();
        }
    }

    #[inline(always)]
    pub(crate) fn record_ic_miss_getprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_getprop();
        }
    }

    #[inline(always)]
    pub(crate) fn record_ic_hit_setprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_setprop();
        }
    }

    #[inline(always)]
    pub(crate) fn record_ic_miss_setprop(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_setprop();
        }
    }

    #[inline(always)]
    pub(crate) fn record_ic_hit_callmethod(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_hit_callmethod();
        }
    }

    #[inline(always)]
    pub(crate) fn record_ic_miss_callmethod(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_ic_miss_callmethod();
        }
    }

    #[inline(always)]
    pub(crate) fn record_call_vm_fast(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_vm_fast();
        }
    }

    #[inline(always)]
    pub(crate) fn record_call_slow(&self) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_slow();
        }
    }

    /// The ONLY way to account for a native call: bumps the aggregate counter
    /// and the per-name hotspot table together.
    ///
    /// These used to be two calls (`record_call_native` + `record_hotspot_native`)
    /// and the second was missing at 6 of the 11 native call sites — every JIT
    /// helper among them. `bench -v` reported 77 252 native calls with ~1 100
    /// attributed, because the tier that runs 97% of the frames never named what
    /// it called. Keep them fused so a new call site cannot report half.
    ///
    /// `name` is the source-level name when the site knows it (a bound method,
    /// a global). `None` means the site only holds a resolved pointer — the JIT
    /// bakes native addresses at compile time — and the name is recovered from
    /// the builtin registry instead.
    #[inline(always)]
    pub(crate) fn record_call_native(&self, f: varn_types::NativeFn, name: Option<&str>) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_native();
        }
        if let Some(ref h) = self.hotspot_counters {
            // An empty name counts as absent: several paths carry a
            // `(NativeFn, &str)` pair whose name is `""` when the value was
            // synthesized rather than resolved from a declaration.
            let resolved = match name {
                Some(n) if !n.is_empty() => n,
                _ => varn_builtins::native_op_name_by_fn(f).unwrap_or("<nativo sin nombre>"),
            };
            h.borrow_mut().record_native_call(resolved);
        }
    }

    #[inline(always)]
    pub(crate) fn record_hotspot_fn(&self, name: &str, jit: bool) {
        if let Some(ref h) = self.hotspot_counters {
            h.borrow_mut().record_fn_call(name, jit);
        }
    }

    #[inline(always)]
    pub(crate) fn record_hotspot_method(&self, name: &str, jit: bool) {
        if let Some(ref h) = self.hotspot_counters {
            h.borrow_mut().record_method_call(name, jit);
        }
    }

    #[inline(always)]
    pub(crate) fn invoke_native(
        &mut self,
        f: varn_types::NativeFn,
        args: &[VmValue],
    ) -> Result<VmValue, String> {
        if self.hotspot_counters.is_none() {
            return (f)(self as &mut dyn varn_types::NativeCtx, args);
        }
        // Resolved independently of whatever name (if any) the caller passed
        // to the adjacent `record_call_native` — this is a raw function
        // pointer, so the builtin registry is the only source of truth
        // available here, same fallback `record_call_native` itself uses
        // when it has no source-level name. Good enough for timing: the
        // rare case where a closure (not a registered builtin) reaches this
        // path buckets under "<nativo sin nombre>" instead of losing the ns.
        let name = varn_builtins::native_op_name_by_fn(f).unwrap_or("<nativo sin nombre>");

        #[cfg(target_arch = "x86_64")]
        {
            static CYCLES_PER_NS: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
            let cycles_per_ns = *CYCLES_PER_NS.get_or_init(|| {
                let start_time = std::time::Instant::now();
                let start_cycles = unsafe { std::arch::x86_64::_rdtsc() };
                let sleep_dur = std::time::Duration::from_millis(2);
                while start_time.elapsed() < sleep_dur {
                    std::hint::spin_loop();
                }
                let elapsed_ns = start_time.elapsed().as_nanos() as f64;
                let elapsed_cycles = (unsafe { std::arch::x86_64::_rdtsc() } - start_cycles) as f64;
                let val = elapsed_cycles / elapsed_ns;
                if val > 0.01 && val < 100.0 {
                    val
                } else {
                    2.5
                }
            });

            let start = unsafe { std::arch::x86_64::_rdtsc() };
            let r = (f)(self as &mut dyn varn_types::NativeCtx, args);
            let end = unsafe { std::arch::x86_64::_rdtsc() };
            let ns = ((end.saturating_sub(start)) as f64 / cycles_per_ns) as u64;
            if let Some(ref h) = self.hotspot_counters {
                h.borrow_mut().record_native_ns(name, ns);
            }
            r
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            let start = std::time::Instant::now();
            let r = (f)(self as &mut dyn varn_types::NativeCtx, args);
            let ns = start.elapsed().as_nanos() as u64;
            if let Some(ref h) = self.hotspot_counters {
                h.borrow_mut().record_native_ns(name, ns);
            }
            r
        }
    }

    #[inline(always)]
    pub(crate) fn record_hotspot_global(&self, idx: usize) {
        if let Some(ref h) = self.hotspot_counters {
            if let Some(name) = self.globals.idx_to_name.get(idx) {
                h.borrow_mut().record_global_access(name.clone());
            }
        }
    }
}
