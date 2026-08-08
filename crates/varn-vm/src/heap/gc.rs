use crate::nursery::{is_nursery_idx, is_old_idx, old_idx_raw, Nursery};
use crate::value::VmValue;
use super::obj::HeapObj;
use super::structs::HeapInner;

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
        let live_count = (self.objects.len() - self.free.len()) as u64;
        self.gc_alloc_since_collect >= 16384.max(live_count)
    }

    pub(crate) fn compact_interners(&mut self) {
        self.string_interner.retain(|_, &mut packed| {
            let raw = old_idx_raw(packed);
            self.objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
        });

        let check = |packed: u32, objects: &Vec<Option<HeapObj>>| -> bool {
            let raw = old_idx_raw(packed);
            objects
                .get(raw as usize)
                .map(|o| o.is_some())
                .unwrap_or(false)
        };

        self.symbol_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.char_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.bigint_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.decimal_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.array_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.object_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.map_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.set_interner
            .retain(|_, &mut packed| check(packed, &self.objects));
        self.identity_index
            .retain(|_, &mut packed| check(packed, &self.objects));
    }

    pub(crate) fn collect(&mut self, roots: &[u32]) -> Result<usize, crate::gc::GcError> {
        if let Some(mut collector) = self.gc_collector.take() {
            let freed = collector.collect(self, roots)?;
            self.gc_collector = Some(collector);
            self.compact_interners();
            self.rebuild_scan_roots();
            self.gc_collections += 1;
            self.gc_total_freed += freed as u64;
            self.gc_alloc_since_collect = 0;
            Ok(freed)
        } else {
            Ok(0)
        }
    }

    pub(crate) fn live_count(&self) -> usize {
        self.objects.len().saturating_sub(self.free.len())
    }
}
