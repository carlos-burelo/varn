//! Minor collection: mark the young objects reachable from the roots, from
//! old objects the barrier remembered and from old objects whose children no
//! barrier covers, then free the unmarked young cells. Survivors become old
//! where they stand; no reference is rewritten.

use super::cells::{CellSpace, SlotState};
use super::children::{for_each_child, Reach};
use super::structs::HeapInner;
use crate::value::VmValue;
use varn_types::HeapRef;

impl HeapInner {
    pub(crate) fn minor_gc(&mut self, roots: &[HeapRef]) {
        let trace = crate::gc_trace::note_start(self.young.len(), self.young.minor_gc_promoted);
        self.young.minor_gc_count += 1;
        let mut work = std::mem::take(&mut self.young.worklist);
        work.clear();

        for &root in roots {
            CellSpace::mark_young(root, &mut work);
        }

        let mut remembered = std::mem::take(&mut self.young.remembered);
        for &r in &remembered {
            if self.cells.state(r) == SlotState::Remembered {
                self.cells.set_state(r, SlotState::Old);
            }
            self.mark_children(r, &mut work);
        }
        remembered.clear();
        self.young.remembered = remembered;

        let scan_roots = std::mem::take(&mut self.scan_roots);
        for &r in &scan_roots {
            self.mark_children(r, &mut work);
        }
        self.scan_roots = scan_roots;

        for cell in std::mem::take(&mut self.young_cells) {
            cell.trace_cells(&mut |c| mark_young_value(c.get(), &mut work));
        }
        for lazy in std::mem::take(&mut self.young_lazies) {
            lazy.trace_cells(&mut |c| mark_young_value(c.get(), &mut work));
        }

        while let Some(r) = work.pop() {
            self.mark_children(r, &mut work);
        }
        self.young.worklist = work;

        self.sweep_young();
        crate::gc_trace::note_end(
            trace,
            self.young.minor_gc_count,
            self.young.minor_gc_promoted,
        );
    }

    fn mark_children(&self, r: HeapRef, work: &mut Vec<HeapRef>) {
        for_each_child(
            self.cells.get(r),
            &self.identity_index,
            Reach::Minor,
            &mut |child| CellSpace::mark_young(child, work),
        );
    }

    fn sweep_young(&mut self) {
        let mut born = std::mem::take(&mut self.young.born);
        for &r in &born {
            match self.cells.state(r) {
                SlotState::Marked => {
                    self.cells.promote(r);
                    self.young.minor_gc_promoted += 1;
                    let obj = self.cells.get(r);
                    let track = Self::needs_minor_scan(obj);
                    let identity = Self::identity_key(obj);
                    if track {
                        self.scan_roots.push(r);
                    }
                    if let Some(key) = identity {
                        self.identity_index.insert(key, r);
                    }
                }
                SlotState::Young => self.cells.release(r),
                SlotState::Old | SlotState::Remembered | SlotState::Free => {}
            }
        }
        self.young.retired += born.len() as u64;
        born.clear();
        self.young.born = born;
    }
}

#[inline]
fn mark_young_value(v: VmValue, work: &mut Vec<HeapRef>) {
    if v.is_heap() {
        CellSpace::mark_young(v.as_heap(), work);
    }
}
