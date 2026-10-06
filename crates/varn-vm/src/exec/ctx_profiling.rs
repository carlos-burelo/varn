use crate::value::VmValue;

use super::ctx::ExecCtx;

impl ExecCtx {
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

    #[inline(always)]
    pub(crate) fn record_call_native(&self, f: varn_types::NativeFn, name: Option<&str>) {
        if let Some(ref c) = self.profile_counters {
            c.record_call_native();
        }
        if let Some(ref h) = self.hotspot_counters {
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
    pub(super) fn timed_native(
        &mut self,
        f: varn_types::NativeFn,
        args: &[VmValue],
    ) -> varn_types::NativeFnResult {
        if crate::home_trace::enabled() {
            let name = varn_builtins::native_op_name_by_fn(f).unwrap_or("<anon>");
            let kinds: Vec<String> = args
                .iter()
                .map(|v| {
                    let k = if v.is_sso() {
                        ":sso"
                    } else if v.is_heap() {
                        match self.heap.get(v.as_heap()) {
                            Some(crate::heap::HeapObj::Str(_)) => ":str",
                            Some(crate::heap::HeapObj::Array(_)) => ":array",
                            Some(crate::heap::HeapObj::Object(_)) => ":object",
                            Some(crate::heap::HeapObj::Instance(_)) => ":instance",
                            Some(_) => ":heap-other",
                            None => ":heap-none",
                        }
                    } else {
                        ""
                    };
                    format!("{:#x}/{:#x}{k}", v.raw_tag(), v.raw_payload())
                })
                .collect();
            eprintln!(
                "INVOKE_NATIVE {name} f={:#x} nargs={} {kinds:?}",
                f as usize,
                args.len()
            );
        }
        if self.hotspot_counters.is_none() {
            return (f)(self as &mut dyn varn_types::NativeCtx, args);
        }

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
            if let Some(name) = self.globals_ref().idx_to_name.get(idx) {
                h.borrow_mut().record_global_access(name.clone());
            }
        }
    }
}
