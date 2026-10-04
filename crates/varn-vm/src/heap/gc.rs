use super::slots::SlotState;
use super::structs::HeapInner;
use crate::value::VmValue;

impl HeapInner {
    pub(crate) fn rebuild_scan_roots(&mut self) {
        self.scan_roots.clear();
        for (idx, obj, _) in self.slots.iter() {
            if Self::needs_minor_scan(obj) {
                self.scan_roots.push(idx);
            }
        }
    }

    #[inline(always)]
    pub(crate) fn needs_minor_gc(&self) -> bool {
        self.young.is_full()
    }

    #[inline(always)]
    pub(crate) fn is_young(&self, v: VmValue) -> bool {
        v.is_heap() && self.slots.state(v.as_heap_idx()) == SlotState::Young
    }

    #[inline(always)]
    pub(crate) fn write_barrier(&mut self, parent: u32, new_val: VmValue) {
        if self.slots.state(parent) == SlotState::Old && self.is_young(new_val) {
            self.slots.set_state(parent, SlotState::Remembered);
            self.young.remembered.push(parent);
        }
    }

    #[inline(always)]
    pub(crate) fn needs_gc(&self) -> bool {
        self.slots.old_growth >= self.gc_threshold
    }

    pub(crate) fn compact_interners(&mut self) {
        let slots = &self.slots;
        let live = |idx: &mut u32| slots.is_live(*idx);
        self.string_interner.retain(|_, idx| live(idx));
        self.symbol_interner.retain(|_, idx| live(idx));
        self.char_interner.retain(|_, idx| live(idx));
        self.bigint_interner.retain(|_, idx| live(idx));
        self.decimal_interner.retain(|_, idx| live(idx));
        self.identity_index.retain(|_, idx| live(idx));
    }

    /// Run a full collection over every slot, young and old alike, returning
    /// how many were freed. Every survivor ends it old, so the young
    /// generation starts empty.
    pub(crate) fn collect(&mut self, roots: &[u32]) -> usize {
        let freed = self.mark_and_sweep(roots);
        self.young.born.clear();
        self.young.remembered.clear();
        self.young_cells.clear();
        self.young_lazies.clear();
        self.compact_interners();
        self.rebuild_scan_roots();
        self.gc_collections += 1;
        self.gc_total_freed += freed as u64;
        self.slots.old_growth = 0;
        let live = self.live_count() as u64;
        self.gc_threshold = (live * 2).max(65536);
        freed
    }

    pub(crate) fn live_count(&self) -> usize {
        self.slots.live_count()
    }
}
