use super::ctx::ExecCtx;

impl ExecCtx {
    /// Loop back-edge GC safepoint shared by the interpreter and the JIT.
    ///
    /// Suspended generator state is traced by the collectors themselves
    /// (`scan_and_fix_old_obj`'s Generator arm / the marker's Generator arm),
    /// so async liveness no longer defers collection. Only nested contexts
    /// (`gc_inhibited`) never initiate one — see that field's invariant.
    pub(crate) fn gc_backedge_safepoint(&mut self) {
        // Verifica el contrato v2 en debug: rangos contiguos, topes >= bases.
        // Costo cero en release.
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

    pub(crate) fn trigger_gc(&mut self) {
        if self.gc_inhibited {
            return;
        }
        self.run_minor_gc();
        let roots = self.major_roots();
        let _ = self.heap.collect(&roots);
    }

    /// Every heap index the context itself holds: the one root set a major
    /// collection starts from, wherever it is triggered.
    pub(crate) fn major_roots(&self) -> Vec<u32> {
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        let scope = super::scheduler::gc_scope(self);
        for &owner in &scope.owners[1..] {
            roots.extend(unsafe { &*owner }.major_roots_local());
        }
        for cell in &scope.cells {
            cell.trace_cells(&mut |c| {
                let v = c.get();
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            });
        }
        for lazy in &scope.lazies {
            lazy.trace_cells(&mut |c| {
                let v = c.get();
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            });
        }
        for &frozen in &scope.frozen {
            let st: &super::scheduler::Frozen = unsafe { &*frozen };
            if !st.has_refs {
                continue;
            }
            for sf in st.frames() {
                for v in &sf.dyn_ {
                    if v.is_heap() {
                        roots.push(v.as_heap_idx());
                    }
                }
                for &h in &sf.refs {
                    if h != crate::frame_store::REF_UNINIT {
                        roots.push(h);
                    }
                }
            }
        }
        roots.extend(self.major_roots_local());
        for v in &self.globals_ref().values {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for v in self.modules_ref().values() {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for (_, v) in unsafe { &*self.static_closures.get() }.values() {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for (_, pool) in unsafe { &*self.proto_constants.get() }.values() {
            for v in pool.iter() {
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            }
        }
        for map in unsafe { &*self.metadata.get() }.values() {
            for &v in map.values() {
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            }
        }
        for reps in unsafe { &*self.hashable_keys.get() }.values() {
            for v in reps {
                if v.is_heap() {
                    roots.push(v.as_heap_idx());
                }
            }
        }
        roots
    }

    fn major_roots_local(&self) -> Vec<u32> {
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        self.stack
            .collect_roots(self.stack.dyn_.len(), self.stack.refs.len(), &mut roots);
        for v in &self.stage {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        if self.jit_native_result.is_heap() {
            roots.push(self.jit_native_result.as_heap_idx());
        }
        self.for_each_jit_slot(|slot| {
            let v = unsafe { *slot };
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        });
        for frame in &self.frames {
            for c in frame.closure().constants.iter() {
                if c.is_heap() {
                    roots.push(c.as_heap_idx());
                }
            }
        }
        for (_, v) in &self.pending_constructors {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for (_, v) in &self.pending_setters {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        roots
    }
}

#[cfg(test)]
mod gc_pool_tests {
    use super::*;
    use crate::globals::GlobalStore;
    use std::rc::Rc;

    #[test]
    fn minor_gc_rewrites_cached_pools() {
        let mut ctx = ExecCtx::new(
            GlobalStore::default(),
            crate::settings::ExecSettings::default(),
        );
        let s = ctx.heap.alloc_str_interned("pool-gc-probe-fresh-string");
        let before = s.as_heap_idx();
        let proto = Rc::new(varn_types::FunctionProto::default());
        unsafe { &mut *ctx.proto_constants.get() }.insert(0x1234, (proto, Rc::new(vec![s])));
        ctx.run_minor_gc();
        let (_, pool) = &unsafe { &*ctx.proto_constants.get() }[&0x1234];
        let after = pool[0].as_heap_idx();
        assert!(ctx.heap.get(after).is_some());
        let _ = before;
    }

    #[test]
    fn major_roots_cover_cached_pools() {
        let mut ctx = ExecCtx::new(
            GlobalStore::default(),
            crate::settings::ExecSettings::default(),
        );
        let s = ctx.heap.alloc_str_interned("pool-gc-probe-major-string");
        let proto = Rc::new(varn_types::FunctionProto::default());
        unsafe { &mut *ctx.proto_constants.get() }.insert(0x1234, (proto, Rc::new(vec![s])));
        let roots = ctx.major_roots();
        assert!(roots.contains(&s.as_heap_idx()));
    }
}
