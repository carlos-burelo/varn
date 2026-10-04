//! Minor collection: mark the young objects reachable from the roots, from
//! old objects the barrier remembered and from old objects whose children no
//! barrier covers, then free the unmarked young slots. Survivors become old
//! where they stand; no handle is rewritten.

use super::children::{for_each_child, Reach};
use super::slots::SlotState;
use super::structs::HeapInner;
use crate::value::VmValue;

impl HeapInner {
    pub(crate) fn minor_gc(&mut self, roots: &[u32]) {
        let trace = crate::gc_trace::note_start(self.young.len(), self.young.minor_gc_promoted);
        self.young.minor_gc_count += 1;
        let mut work = std::mem::take(&mut self.young.worklist);
        work.clear();

        for &root in roots {
            self.mark_young(root, &mut work);
        }

        let mut remembered = std::mem::take(&mut self.young.remembered);
        for &idx in &remembered {
            if self.slots.state(idx) == SlotState::Remembered {
                self.slots.set_state(idx, SlotState::Old);
            }
            self.mark_children(idx, &mut work);
        }
        remembered.clear();
        self.young.remembered = remembered;

        let scan_roots = std::mem::take(&mut self.scan_roots);
        for &idx in &scan_roots {
            self.mark_children(idx, &mut work);
        }
        self.scan_roots = scan_roots;

        for cell in std::mem::take(&mut self.young_cells) {
            cell.trace_cells(&mut |c| self.mark_young_value(c.get(), &mut work));
        }
        for lazy in std::mem::take(&mut self.young_lazies) {
            lazy.trace_cells(&mut |c| self.mark_young_value(c.get(), &mut work));
        }

        while let Some(idx) = work.pop() {
            self.mark_children(idx, &mut work);
        }
        self.young.worklist = work;

        self.sweep_young();
        crate::gc_trace::note_end(
            trace,
            self.young.minor_gc_count,
            self.young.minor_gc_promoted,
        );
    }

    #[inline]
    fn mark_young(&mut self, idx: u32, work: &mut Vec<u32>) {
        mark_in(self.slots.split_at(idx).1, idx, work);
    }

    fn mark_young_value(&mut self, v: VmValue, work: &mut Vec<u32>) {
        if v.is_heap() {
            self.mark_young(v.as_heap_idx(), work);
        }
    }

    fn mark_children(&mut self, idx: u32, work: &mut Vec<u32>) {
        let identity = &self.identity_index;
        let (obj, states) = self.slots.split_at(idx);
        if let Some(obj) = obj {
            for_each_child(obj, identity, Reach::Minor, &mut |child| {
                mark_in(states, child, work)
            });
        }
    }

    fn sweep_young(&mut self) {
        let mut born = std::mem::take(&mut self.young.born);
        for &idx in &born {
            match self.slots.state(idx) {
                SlotState::Marked => {
                    self.slots.promote(idx);
                    self.young.minor_gc_promoted += 1;
                    if let Some(obj) = self.slots.get(idx) {
                        let track = Self::needs_minor_scan(obj);
                        let identity = Self::identity_key(obj);
                        if track {
                            self.scan_roots.push(idx);
                        }
                        if let Some(key) = identity {
                            self.identity_index.insert(key, idx);
                        }
                    }
                }
                SlotState::Young => {
                    self.slots.release(idx);
                }
                SlotState::Old | SlotState::Remembered => {}
            }
        }
        born.clear();
        self.young.born = born;
    }
}

#[inline]
fn mark_in(states: &mut [SlotState], idx: u32, work: &mut Vec<u32>) {
    if let Some(state) = states.get_mut(idx as usize) {
        if *state == SlotState::Young {
            *state = SlotState::Marked;
            work.push(idx);
        }
    }
}
