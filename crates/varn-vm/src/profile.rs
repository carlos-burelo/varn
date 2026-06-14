use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use rustc_hash::FxHashMap;

pub struct ProfileCounters {
    pub ic_hits: AtomicU64,
    pub ic_misses: AtomicU64,
    pub ic_hits_getprop: AtomicU64,
    pub ic_misses_getprop: AtomicU64,
    pub ic_hits_setprop: AtomicU64,
    pub ic_misses_setprop: AtomicU64,
    pub ic_hits_callmethod: AtomicU64,
    pub ic_misses_callmethod: AtomicU64,
    pub calls_vm_fast: AtomicU64,
    pub calls_prepare_slow: AtomicU64,
    pub calls_native: AtomicU64,
    pub heap_allocs: AtomicU64,
    pub frame_pushes: AtomicU64,
    pub frame_pops: AtomicU64,
}

impl ProfileCounters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            ic_hits: AtomicU64::new(0),
            ic_misses: AtomicU64::new(0),
            ic_hits_getprop: AtomicU64::new(0),
            ic_misses_getprop: AtomicU64::new(0),
            ic_hits_setprop: AtomicU64::new(0),
            ic_misses_setprop: AtomicU64::new(0),
            ic_hits_callmethod: AtomicU64::new(0),
            ic_misses_callmethod: AtomicU64::new(0),
            calls_vm_fast: AtomicU64::new(0),
            calls_prepare_slow: AtomicU64::new(0),
            calls_native: AtomicU64::new(0),
            heap_allocs: AtomicU64::new(0),
            frame_pushes: AtomicU64::new(0),
            frame_pops: AtomicU64::new(0),
        })
    }

    #[inline(always)]
    pub fn record_ic_hit(&self) {
        self.ic_hits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_ic_miss(&self) {
        self.ic_misses.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_ic_hit_getprop(&self) {
        self.ic_hits.fetch_add(1, Ordering::Relaxed);
        self.ic_hits_getprop.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_ic_miss_getprop(&self) {
        self.ic_misses.fetch_add(1, Ordering::Relaxed);
        self.ic_misses_getprop.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_ic_hit_setprop(&self) {
        self.ic_hits.fetch_add(1, Ordering::Relaxed);
        self.ic_hits_setprop.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_ic_miss_setprop(&self) {
        self.ic_misses.fetch_add(1, Ordering::Relaxed);
        self.ic_misses_setprop.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_ic_hit_callmethod(&self) {
        self.ic_hits.fetch_add(1, Ordering::Relaxed);
        self.ic_hits_callmethod.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_ic_miss_callmethod(&self) {
        self.ic_misses.fetch_add(1, Ordering::Relaxed);
        self.ic_misses_callmethod.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_call_vm_fast(&self) {
        self.calls_vm_fast.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_call_slow(&self) {
        self.calls_prepare_slow.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_call_native(&self) {
        self.calls_native.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_heap_alloc(&self) {
        self.heap_allocs.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_frame_push(&self) {
        self.frame_pushes.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_frame_pop(&self) {
        self.frame_pops.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub struct VmProfile {
    pub ic_hits: u64,
    pub ic_misses: u64,
    pub ic_hits_getprop: u64,
    pub ic_misses_getprop: u64,
    pub ic_hits_setprop: u64,
    pub ic_misses_setprop: u64,
    pub ic_hits_callmethod: u64,
    pub ic_misses_callmethod: u64,
    pub calls_vm_fast: u64,
    pub calls_prepare_slow: u64,
    pub calls_native: u64,
    pub heap_allocs: u64,
    pub move_opcodes: u64,
    pub frame_pushes: u64,
    pub frame_pops: u64,
    pub gc_collections: u64,
    pub gc_freed: u64,
    pub heap_live: u64,
    pub heap_total: u64,
    pub nursery_allocs: u64,
    pub minor_gc_count: u64,
    pub minor_gc_promoted: u64,
}

impl VmProfile {
    pub fn from_counters(c: &ProfileCounters) -> Self {
        Self {
            ic_hits: c.ic_hits.load(Ordering::Relaxed),
            ic_misses: c.ic_misses.load(Ordering::Relaxed),
            ic_hits_getprop: c.ic_hits_getprop.load(Ordering::Relaxed),
            ic_misses_getprop: c.ic_misses_getprop.load(Ordering::Relaxed),
            ic_hits_setprop: c.ic_hits_setprop.load(Ordering::Relaxed),
            ic_misses_setprop: c.ic_misses_setprop.load(Ordering::Relaxed),
            ic_hits_callmethod: c.ic_hits_callmethod.load(Ordering::Relaxed),
            ic_misses_callmethod: c.ic_misses_callmethod.load(Ordering::Relaxed),
            calls_vm_fast: c.calls_vm_fast.load(Ordering::Relaxed),
            calls_prepare_slow: c.calls_prepare_slow.load(Ordering::Relaxed),
            calls_native: c.calls_native.load(Ordering::Relaxed),
            heap_allocs: c.heap_allocs.load(Ordering::Relaxed),
            move_opcodes: 0,
            frame_pushes: c.frame_pushes.load(Ordering::Relaxed),
            frame_pops: c.frame_pops.load(Ordering::Relaxed),
            gc_collections: 0,
            gc_freed: 0,
            heap_live: 0,
            heap_total: 0,
            nursery_allocs: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallEntry {
    pub calls: u64,
    pub jit_calls: u64,
    pub interp_calls: u64,
}

#[derive(Debug, Clone, Default)]
pub struct HotspotCounters {
    pub fn_calls: FxHashMap<Rc<str>, CallEntry>,
    pub method_calls: FxHashMap<Rc<str>, CallEntry>,
    pub native_calls: FxHashMap<Rc<str>, u64>,
    /// Cumulative wall-time (ns) spent inside native builtins — the actual cost,
    /// which call counts alone don't reveal. Only populated in profiling mode
    /// (hotspot counters enabled, i.e. `vn bench`).
    pub total_native_ns: u64,
    pub global_accesses: FxHashMap<Rc<str>, u64>,
    pub alloc_types: FxHashMap<&'static str, u64>,
}

impl HotspotCounters {
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self::default()))
    }

    pub fn record_fn_call(&mut self, name: &str, jit: bool) {
        let e = self.fn_calls.entry(Rc::from(name)).or_default();
        e.calls += 1;
        if jit {
            e.jit_calls += 1;
        } else {
            e.interp_calls += 1;
        }
    }

    pub fn record_method_call(&mut self, name: &str, jit: bool) {
        let e = self.method_calls.entry(Rc::from(name)).or_default();
        e.calls += 1;
        if jit {
            e.jit_calls += 1;
        } else {
            e.interp_calls += 1;
        }
    }

    pub fn record_native_call(&mut self, name: &str) {
        *self.native_calls.entry(Rc::from(name)).or_default() += 1;
    }

    pub fn record_global_access(&mut self, name: Rc<str>) {
        *self.global_accesses.entry(name).or_default() += 1;
    }

    pub fn record_alloc(&mut self, type_name: &'static str) {
        *self.alloc_types.entry(type_name).or_default() += 1;
    }
}
