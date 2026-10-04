//! When the collectors run and what they start from. Both collections share
//! one root set: objects never move, so a root is only ever read.

use super::ctx::ExecCtx;
use super::VmSuspend;

impl ExecCtx {
    /// Loop back-edge GC safepoint shared by the interpreter and the JIT.
    /// Only nested contexts (`gc_inhibited`) never initiate a collection.
    pub(crate) fn gc_backedge_safepoint(&mut self) {
        debug_assert!(crate::frame_store_abi::debug_check_stacks(&self.stack));
        if self.gc_inhibited {
            return;
        }
        if self.heap.needs_minor_gc() {
            self.run_minor_gc();
        }
        if self.heap.needs_gc() {
            self.trigger_gc();
        }
    }

    pub fn run_minor_gc(&mut self) {
        let roots = self.gc_roots();
        self.heap.minor_gc(&roots);
    }

    pub(crate) fn trigger_gc(&mut self) -> usize {
        if self.gc_inhibited {
            return 0;
        }
        let roots = self.gc_roots();
        self.heap.collect(&roots)
    }

    /// Every heap index this context and every context sharing its heap
    /// hold: queued forks, frozen tasks, task cells and the global tables.
    pub(crate) fn gc_roots(&self) -> Vec<u32> {
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        let value = |roots: &mut Vec<u32>, v: crate::value::VmValue| {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        };
        let scope = super::scheduler::gc_scope(self);
        for &owner in &scope.owners[1..] {
            unsafe { &*owner }.local_roots(&mut roots);
        }
        for cell in &scope.cells {
            cell.trace_cells(&mut |c| value(&mut roots, c.get()));
        }
        for lazy in &scope.lazies {
            lazy.trace_cells(&mut |c| value(&mut roots, c.get()));
        }
        for &frozen in &scope.frozen {
            let st: &super::scheduler::Frozen = unsafe { &*frozen };
            if !st.has_refs {
                continue;
            }
            for sf in st.frames() {
                sf.dyn_.iter().for_each(|&v| value(&mut roots, v));
                for &h in &sf.refs {
                    if h != crate::frame_store::REF_UNINIT {
                        roots.push(h);
                    }
                }
            }
        }
        self.local_roots(&mut roots);
        for &v in &self.globals_ref().values {
            value(&mut roots, v);
        }
        for &v in self.modules_ref().values() {
            value(&mut roots, v);
        }
        for (_, v) in unsafe { &*self.static_closures.get() }.values() {
            value(&mut roots, *v);
        }
        for (_, pool) in unsafe { &*self.proto_constants.get() }.values() {
            pool.iter().for_each(|&v| value(&mut roots, v));
        }
        for map in unsafe { &*self.metadata.get() }.values() {
            map.values().for_each(|&v| value(&mut roots, v));
        }
        for reps in unsafe { &*self.hashable_keys.get() }.values() {
            reps.iter().for_each(|&v| value(&mut roots, v));
        }
        roots
    }

    fn local_roots(&self, roots: &mut Vec<u32>) {
        let mut value = |v: crate::value::VmValue| {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        };
        self.stage.iter().for_each(|&v| value(v));
        value(self.jit_native_result);
        self.module_exports.values().for_each(|&v| value(v));
        if let Some(VmSuspend::Yield { value: v, .. } | VmSuspend::Await { value: v, .. }) =
            &self.vm_suspend
        {
            value(*v);
        }
        self.for_each_jit_slot(|slot| value(unsafe { *slot }));
        for frame in &self.frames {
            frame.closure().constants.iter().for_each(|&c| value(c));
        }
        for (_, v) in &self.pending_constructors {
            value(*v);
        }
        for (_, v) in &self.pending_setters {
            value(*v);
        }
        self.stack
            .collect_roots(self.stack.dyn_.len(), self.stack.refs.len(), roots);
    }
}
