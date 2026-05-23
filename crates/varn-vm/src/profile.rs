use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
    pub reg_loads: AtomicU64,
    pub reg_stores: AtomicU64,
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
            reg_loads: AtomicU64::new(0),
            reg_stores: AtomicU64::new(0),
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
    pub fn record_reg_load(&self) {
        self.reg_loads.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub fn record_reg_store(&self) {
        self.reg_stores.fetch_add(1, Ordering::Relaxed);
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
    pub reg_loads: u64,
    pub reg_stores: u64,
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
            reg_loads: c.reg_loads.load(Ordering::Relaxed),
            reg_stores: c.reg_stores.load(Ordering::Relaxed),
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
