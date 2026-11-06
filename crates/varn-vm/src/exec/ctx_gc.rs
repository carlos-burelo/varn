use super::ctx::ExecCtx;
use super::VmSuspend;
use varn_types::HeapRef;

impl ExecCtx {
    pub(crate) fn gc_backedge_safepoint(&mut self) {
        debug_assert!(crate::frame_store_abi::debug_check_stacks(&self.stack));
        if self.heap.needs_minor_gc() {
            self.run_minor_gc();
        }
        if self.heap.needs_gc() {
            self.trigger_gc();
        }
    }

    #[inline(always)]
    pub(crate) fn invoke_native(
        &mut self,
        f: varn_types::NativeFn,
        args: &[crate::value::VmValue],
    ) -> varn_types::NativeFnResult {
        varn_builtins::check_native_fn_capability(f, &*self)?;
        let scope = self.heap.cells.enter_native();
        let result = self.timed_native(f, args);
        self.heap.cells.exit_native(scope);
        result
    }

    pub fn run_minor_gc(&mut self) {
        let _collecting = crate::cpu_profile::Activity::collecting();
        let start = std::time::Instant::now();
        let roots = self.gc_roots();
        self.heap.minor_gc(&roots);
        self.heap.gc_time.add_minor(start.elapsed());
    }

    pub(crate) fn trigger_gc(&mut self) -> usize {
        let _collecting = crate::cpu_profile::Activity::collecting();
        let start = std::time::Instant::now();
        let roots = self.gc_roots();
        let freed = self.heap.collect(&roots);
        self.heap.gc_time.add_major(start.elapsed());
        freed
    }

    pub(crate) fn gc_roots(&self) -> Vec<HeapRef> {
        let mut roots: Vec<HeapRef> = Vec::with_capacity(256);
        let value = |roots: &mut Vec<HeapRef>, v: crate::value::VmValue| {
            if v.is_heap() {
                roots.push(v.as_heap());
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
        for state in &scope.gens {
            state.trace(&mut |v| value(&mut roots, v));
        }
        for &frozen in &scope.frozen {
            let st: &super::scheduler::Frozen = unsafe { &*frozen };
            st.trace(&mut |v| value(&mut roots, v));
        }
        self.local_roots(&mut roots);
        roots.extend_from_slice(self.heap.cells.native_roots());
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

    fn local_roots(&self, roots: &mut Vec<HeapRef>) {
        let mut value = |v: crate::value::VmValue| {
            if v.is_heap() {
                roots.push(v.as_heap());
            }
        };
        self.stage.iter().for_each(|&v| value(v));
        value(self.jit_native_result);
        self.module_exports.values().for_each(|&v| value(v));
        match &self.vm_suspend {
            Some(VmSuspend::Yield { value: v, .. }) => value(*v),
            Some(VmSuspend::Await { cell, .. }) => cell.trace_cells(&mut |c| value(c.get())),
            Some(VmSuspend::DebugBreak) | None => {}
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
