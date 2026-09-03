use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::nursery::{is_nursery_idx, is_old_idx, old_idx_raw, Nursery};
use crate::value::VmValue;

impl HeapInner {
    pub(crate) fn rebuild_scan_roots(&mut self) {
        self.scan_roots.clear();
        for (idx, obj) in self.objects.iter().enumerate() {
            if let Some(obj) = obj {
                if Self::needs_minor_scan(obj) {
                    self.scan_roots.push(idx as u32);
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn needs_minor_gc(&self) -> bool {
        self.nursery.is_full()
    }

    #[inline(always)]
    pub(crate) fn write_barrier(&mut self, parent_packed_idx: u32, new_val: VmValue) {
        if is_old_idx(parent_packed_idx)
            && new_val.is_heap()
            && is_nursery_idx(new_val.as_heap_idx())
        {
            self.nursery.remember(parent_packed_idx);
        }
    }

    pub(crate) fn minor_gc(&mut self, stack: &mut [VmValue], extra_packed: &[u32]) {
        let mut nursery = std::mem::replace(&mut self.nursery, Nursery::vacant());
        nursery.collect(self, stack, extra_packed);
        self.nursery = nursery;
    }

    #[inline(always)]
    pub(crate) fn needs_gc(&self) -> bool {
        self.gc_alloc_since_collect >= self.gc_threshold
    }

    pub(crate) fn compact_interners(&mut self) {
        if !self.string_interner.is_empty() {
            self.string_interner.retain(|_, &mut packed| {
                let raw = old_idx_raw(packed);
                self.objects
                    .get(raw as usize)
                    .map(|o| o.is_some())
                    .unwrap_or(false)
            });
        }

        let check = |packed: u32, objects: &Vec<Option<HeapObj>>| -> bool {
            let raw = old_idx_raw(packed);
            objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
        };

        if !self.symbol_interner.is_empty() {
            self.symbol_interner
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
        if !self.char_interner.is_empty() {
            self.char_interner
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
        if !self.bigint_interner.is_empty() {
            self.bigint_interner
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
        if !self.decimal_interner.is_empty() {
            self.decimal_interner
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
        if !self.array_interner.is_empty() {
            self.array_interner
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
        if !self.object_interner.is_empty() {
            self.object_interner
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
        if !self.map_interner.is_empty() {
            self.map_interner
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
        if !self.set_interner.is_empty() {
            self.set_interner
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
        if !self.identity_index.is_empty() {
            self.identity_index
                .retain(|_, &mut packed| check(packed, &self.objects));
        }
    }

    pub(crate) fn update_interners_after_minor_gc(&mut self, fwd: &[Option<u32>]) {
        if fwd.is_empty() {
            return;
        }
        let update_interner = |packed: &mut u32| -> bool {
            if crate::nursery::is_nursery_idx(*packed) {
                if let Some(Some(new_packed)) = fwd.get(*packed as usize) {
                    *packed = *new_packed;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        };

        if self
            .object_interner
            .values()
            .any(|&p| crate::nursery::is_nursery_idx(p))
        {
            self.object_interner.retain(|_, p| update_interner(p));
        }
        if self
            .map_interner
            .values()
            .any(|&p| crate::nursery::is_nursery_idx(p))
        {
            self.map_interner.retain(|_, p| update_interner(p));
        }
        if self
            .set_interner
            .values()
            .any(|&p| crate::nursery::is_nursery_idx(p))
        {
            self.set_interner.retain(|_, p| update_interner(p));
        }
        if self
            .array_interner
            .values()
            .any(|&p| crate::nursery::is_nursery_idx(p))
        {
            self.array_interner.retain(|_, p| update_interner(p));
        }
        if self
            .string_interner
            .values()
            .any(|&p| crate::nursery::is_nursery_idx(p))
        {
            self.string_interner.retain(|_, p| update_interner(p));
        }
        if self
            .symbol_interner
            .values()
            .any(|&p| crate::nursery::is_nursery_idx(p))
        {
            self.symbol_interner.retain(|_, p| update_interner(p));
        }
        if self
            .identity_index
            .values()
            .any(|&p| crate::nursery::is_nursery_idx(p))
        {
            self.identity_index.retain(|_, p| update_interner(p));
        }
    }

    /// Run a full collection, returning how many slots were freed.
    ///
    /// Infallible: marking and sweeping walk structures the heap already owns
    /// and have no failure mode. This used to return `Result<usize, GcError>`
    /// with an error type none of the code could construct, which forced every
    /// caller to handle an impossible case — `Vm::collect_gc` did it with
    /// `.unwrap_or(0)`, which would have silently swallowed a real failure the
    /// day one was introduced.
    pub(crate) fn collect(&mut self, roots: &[u32]) -> usize {
        let Some(mut collector) = self.gc_collector.take() else {
            return 0;
        };
        let freed = collector.collect(self, roots);
        self.gc_collector = Some(collector);
        self.compact_interners();
        self.rebuild_scan_roots();
        self.gc_collections += 1;
        self.gc_total_freed += freed as u64;
        self.gc_alloc_since_collect = 0;
        let live = self.live_count() as u64;
        self.gc_threshold = (live * 2).max(65536);
        freed
    }

    pub(crate) fn live_count(&self) -> usize {
        self.objects.len().saturating_sub(self.free.len())
    }
}
