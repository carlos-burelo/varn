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
            if self.cells.state(r) == SlotState::Free {
                continue;
            }
            if self.cells.state(r) == SlotState::Remembered {
                self.cells.set_state(r, SlotState::Old);
            }
            self.mark_children(r, &mut work);
        }
        remembered.clear();
        self.young.remembered = remembered;

        let mut edges = std::mem::take(&mut self.young.edges);
        for &(parent, child) in &edges {
            if self.cells.state(parent) == SlotState::Free {
                continue;
            }
            CellSpace::mark_young(child, &mut work);
        }

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

        edges.retain(|&(parent, child)| {
            self.cells.state(parent) != SlotState::Free
                && self.cells.state(child) == SlotState::Young
        });
        self.young.edges = edges;

        let grown = varn_types::memory::ExternalLedger::total();
        self.alloc_since_major += self.young.bytes + grown;
        varn_types::memory::ExternalLedger::reset();
        self.recompute_young_limit();
        self.verify_heap(super::verify::Phase::Minor);
        crate::gc_trace::note_end(
            trace,
            self.young.minor_gc_count,
            self.young.minor_gc_promoted,
            self.young.bytes,
            grown,
        );
    }

    fn mark_children(&self, r: HeapRef, work: &mut Vec<HeapRef>) {
        for_each_child(
            &self.cells,
            r,
            &self.identity_index,
            Reach::Minor,
            &mut |child| CellSpace::mark_young(child, work),
        );
    }

    fn sweep_young(&mut self) {
        let mut born = std::mem::take(&mut self.young.born);
        let mut bytes = 0u64;
        for &r in &born {
            bytes += self.cells.cell_bytes(r);
            match self.cells.state(r) {
                SlotState::Marked => {
                    self.cells.promote(r);
                    self.young.minor_gc_promoted += 1;
                    let identity = if self.instance(r).is_some() {
                        None
                    } else {
                        Self::identity_key(self.cells.get(r))
                    };
                    if let Some(key) = identity {
                        self.identity_index.insert(key, r);
                    }
                }
                SlotState::Young => self.cells.release(r),
                SlotState::Old | SlotState::Remembered | SlotState::Free => {}
            }
        }
        self.young.retired += born.len() as u64;
        self.young.bytes = bytes;
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
