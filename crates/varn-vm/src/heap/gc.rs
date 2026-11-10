use super::cells::SlotState;
use super::structs::HeapInner;
use crate::value::VmValue;
use varn_types::HeapRef;

impl HeapInner {
    pub(crate) fn rebuild_scan_roots(&mut self) {
        self.scan_roots.clear();
        for r in self.cells.refs() {
            if self.cells.header_kind(r) == varn_types::cell::CELL_KIND_INSTANCE {
                continue;
            }
            if Self::needs_minor_scan(self.cells.get(r)) {
                self.scan_roots.push(r);
            }
        }
    }

    #[inline(always)]
    pub(crate) fn needs_minor_gc(&self) -> bool {
        self.young.is_full()
    }

    #[inline(always)]
    pub(crate) fn is_young(&self, v: VmValue) -> bool {
        v.is_heap() && self.cells.state(v.as_heap()) == SlotState::Young
    }

    #[inline(always)]
    pub(crate) fn write_barrier(&mut self, parent: HeapRef, new_val: VmValue) {
        if self.cells.state(parent) == SlotState::Old && self.is_young(new_val) {
            self.cells.set_state(parent, SlotState::Remembered);
            self.young.remembered.push(parent);
        }
    }

    #[inline(always)]
    pub(crate) fn needs_gc(&self) -> bool {
        self.alloc_since_major >= self.gc_threshold
    }

    pub(crate) fn recompute_young_limit(&mut self) {
        const MIN_LIMIT: u64 = 1024;
        const UNKNOWN_CELL_BYTES: u64 = 24;
        let avg = if self.young.len() == 0 {
            UNKNOWN_CELL_BYTES
        } else {
            (self.young.bytes / self.young.len() as u64).max(1)
        };
        let limit = (self.nursery_bytes / avg).max(MIN_LIMIT);
        self.young.limit_cells = self.stress_cells.map_or(limit, |cells| limit.min(cells));
    }

    pub(crate) fn compact_interners(&mut self) {
        let cells = &self.cells;
        let live = |r: &mut HeapRef| cells.state(*r) != SlotState::Free;
        self.string_interner.retain(|_, idx| live(idx));
        self.char_interner.retain(|_, idx| live(idx));
        self.bigint_interner.retain(|_, idx| live(idx));
        self.decimal_interner.retain(|_, idx| live(idx));
        self.identity_index.retain(|_, idx| live(idx));
    }

    pub(crate) fn collect(&mut self, roots: &[HeapRef]) -> usize {
        let started = crate::gc_trace::enabled().then(std::time::Instant::now);
        let freed = self.mark_and_sweep(roots);
        self.young.retired += self.young.born.len() as u64;
        self.young.born.clear();
        self.young.remembered.clear();
        self.young_cells.clear();
        self.young_lazies.clear();
        self.young.bytes = 0;
        self.compact_interners();
        self.rebuild_scan_roots();
        self.verify_heap(super::verify::Phase::Major);
        self.gc_collections += 1;
        self.gc_total_freed += freed as u64;
        let alloc_since_major = self.alloc_since_major;
        self.alloc_since_major = 0;
        self.live_bytes_after_major = self.cells.live_bytes;
        self.gc_threshold = (2 * self.live_bytes_after_major).max(2 * self.nursery_bytes);
        varn_types::memory::ExternalLedger::reset();
        self.recompute_young_limit();
        if let Some(started) = started {
            crate::gc_trace::major(
                self.gc_collections,
                freed,
                self.live_bytes_after_major,
                alloc_since_major,
                started.elapsed(),
            );
        }
        freed
    }

    pub(crate) fn live_count(&self) -> usize {
        self.cells.live_count()
    }
}
