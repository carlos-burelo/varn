use super::cells::SlotState;
use super::structs::HeapInner;
use crate::value::VmValue;
use vn_types::HeapRef;

impl HeapInner {
    #[inline(always)]
    pub(crate) fn needs_minor_gc(&self) -> bool {
        self.young.is_full()
    }

    #[inline(always)]
    pub(crate) fn is_young(&self, v: VmValue) -> bool {
        if !v.is_heap() {
            return false;
        }
        matches!(
            self.cells.state(v.as_heap()),
            SlotState::Young | SlotState::Survivor
        )
    }

    #[inline]
    pub(crate) fn remembers_whole(&self, parent: HeapRef) -> bool {
        if self.cells.header_kind(parent) == vn_types::cell::CELL_KIND_INSTANCE {
            return true;
        }
        !matches!(
            self.cells.get(parent),
            super::obj::HeapObj::Map(_) | super::obj::HeapObj::Set(_)
        )
    }

    #[inline(always)]
    pub(crate) fn write_barrier(&mut self, parent: HeapRef, new_val: VmValue) {
        if !new_val.is_heap() {
            return;
        }
        let child = new_val.as_heap();
        if !self.is_young(new_val) {
            return;
        }
        let state = self.cells.state(parent);
        if matches!(state, SlotState::Young | SlotState::Survivor) {
            return;
        }
        const EDGE_CAP: usize = 1 << 16;
        if self.remembers_whole(parent) || self.young.edges.len() >= EDGE_CAP {
            if state != SlotState::Remembered {
                self.cells.set_state(parent, SlotState::Remembered);
                self.young.remembered.push(parent);
            }
        } else {
            self.young.edges.push((parent, child));
        }
    }

    #[inline]
    pub(crate) fn remember(&mut self, parent: HeapRef) {
        let state = self.cells.state(parent);
        if state != SlotState::Young && state != SlotState::Remembered {
            self.cells.set_state(parent, SlotState::Remembered);
            self.young.remembered.push(parent);
        }
    }

    #[inline(always)]
    pub(crate) fn write_upvalue_barrier(&mut self, owner: u64, val: VmValue) {
        if owner != 0 {
            self.write_barrier(unsafe { HeapRef::from_addr_unchecked(owner) }, val);
        }
    }

    #[inline(always)]
    pub(crate) fn needs_gc(&self) -> bool {
        self.cells.old_bytes >= self.gc_threshold
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
        let doomed = |r: &mut HeapRef| cells.state(*r) == SlotState::Free || cells.will_die(*r);
        self.string_interner.retain(|_, idx| !doomed(idx));
        self.char_interner.retain(|_, idx| !doomed(idx));
        self.bigint_interner.retain(|_, idx| !doomed(idx));
        self.decimal_interner.retain(|_, idx| !doomed(idx));
        self.identity_index.retain(|_, idx| !doomed(idx));
    }

    pub(crate) fn collect(&mut self, roots: &[HeapRef]) -> usize {
        let started = crate::gc_trace::enabled().then(std::time::Instant::now);
        let freed = self.mark_and_sweep(roots);
        self.young.retired += self.young.born.len() as u64;
        self.young.born.clear();
        self.young.survivors.clear();
        self.young.remembered.clear();
        self.young.edges.clear();
        self.young_cells.clear();
        self.young_lazies.clear();
        self.young.bytes = 0;
        self.gc_collections += 1;
        self.gc_total_freed += freed as u64;
        self.live_bytes_after_major = self.cells.old_bytes;
        self.gc_threshold = (2 * self.live_bytes_after_major).max(2 * self.nursery_bytes);
        self.recalibrate_threshold = self.cells.dirty_blocks() > 0;
        vn_types::memory::ExternalLedger::reset();
        self.recompute_young_limit();
        self.verify_heap(super::verify::Phase::Major, roots);
        if let Some(started) = started {
            crate::gc_trace::major(
                self.gc_collections,
                freed,
                self.live_bytes_after_major,
                self.gc_threshold,
                started.elapsed(),
            );
        }
        freed
    }

    pub(crate) fn settle_sweeps(&mut self, budget: usize) {
        let done = self.cells.sweep_some(budget);
        if done > 0 {
            crate::gc_trace::sweeps(
                done,
                self.cells.dirty_blocks(),
                self.cells.block_bytes(),
                self.cells.old_bytes,
            );
        }
        self.forget_swept();
        if self.recalibrate_threshold && self.cells.dirty_blocks() == 0 {
            self.recalibrate_threshold = false;
            self.live_bytes_after_major = self.cells.old_bytes;
            self.gc_threshold = (2 * self.live_bytes_after_major).max(2 * self.nursery_bytes);
        }
    }

    pub(crate) fn forget_swept(&mut self) {
        for key in self.cells.take_swept_identities() {
            self.identity_index.remove(&key);
        }
        for key in self.cells.take_swept_interned() {
            match key {
                super::cells::SweptInterned::Str(s) => {
                    self.string_interner.remove(&s);
                }
                super::cells::SweptInterned::Char(c) => {
                    self.char_interner.remove(&c);
                }
                super::cells::SweptInterned::BigInt(n) => {
                    self.bigint_interner.remove(&n);
                }
                super::cells::SweptInterned::Decimal(d) => {
                    self.decimal_interner.remove(&d);
                }
            }
        }
    }

    pub(crate) fn live_count(&self) -> usize {
        self.cells.live_count()
    }
}
