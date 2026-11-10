use super::cells::{CellSpace, SlotState};
use super::children::for_each_child;
use super::structs::HeapInner;
use crate::value::VmValue;
use varn_types::HeapRef;

impl HeapInner {
    pub(crate) fn minor_gc(&mut self, roots: &[HeapRef]) {
        let trace = crate::gc_trace::note_start(self.young.len(), self.young.minor_gc_promoted);
        let mark_started = std::time::Instant::now();
        self.young.minor_gc_count += 1;
        let mut work = std::mem::take(&mut self.young.worklist);
        work.clear();

        for &root in roots {
            CellSpace::mark_young(root, &mut work);
        }

        let mut remembered = std::mem::take(&mut self.young.remembered);
        let mut next_remembered: Vec<HeapRef> = Vec::with_capacity(remembered.len());
        for &r in &remembered {
            if self.cells.state(r) == SlotState::Free {
                continue;
            }
            if self.cells.state(r) == SlotState::Remembered {
                self.cells.set_state(r, SlotState::Old);
            }
            if self.trace_children(r, &mut work) && self.remembers_whole(r) {
                next_remembered.push(r);
            }
        }
        for &r in &remembered {
            if self.cells.state(r) != SlotState::Free {
                CellSpace::clear_traced(r);
            }
        }
        remembered.clear();
        remembered.extend_from_slice(&next_remembered);
        self.young.remembered = remembered;

        let mut edges = std::mem::take(&mut self.young.edges);
        for &(parent, child) in &edges {
            if self.cells.state(parent) == SlotState::Free {
                continue;
            }
            CellSpace::mark_young(child, &mut work);
        }

        let cells = std::mem::take(&mut self.young_cells);
        for cell in &cells {
            cell.trace_cells(&mut |c| mark_young_value(c.get(), &mut work));
        }
        let lazies = std::mem::take(&mut self.young_lazies);
        for lazy in &lazies {
            lazy.trace_cells(&mut |c| mark_young_value(c.get(), &mut work));
        }

        while let Some(r) = work.pop() {
            self.mark_children(r, &mut work);
        }
        self.young.worklist = work;

        let marked = mark_started.elapsed();
        let sweep_started = std::time::Instant::now();
        self.sweep_young();

        self.young_cells = cells
            .into_iter()
            .filter(|cell| {
                let mut young = false;
                cell.trace_cells(&mut |c| young |= self.is_young(c.get()));
                young
            })
            .collect();
        self.young_lazies = lazies
            .into_iter()
            .filter(|lazy| {
                let mut young = false;
                lazy.trace_cells(&mut |c| young |= self.is_young(c.get()));
                young
            })
            .collect();
        if self.verify_gc && !self.young.born.is_empty() {
            panic!(
                "{} cells were born during a minor sweep",
                self.young.born.len()
            );
        }

        edges.retain(|&(parent, child)| {
            self.cells.state(parent) != SlotState::Free && self.is_young(VmValue::from_heap(child))
        });
        edges.append(&mut self.young.edges);
        self.young.edges = edges;

        let grown = varn_types::memory::ExternalLedger::total();
        varn_types::memory::ExternalLedger::reset();
        self.recompute_young_limit();
        let sweep_budget = (self.nursery_bytes / varn_types::cell::BLOCK_BYTES as u64).max(16);
        self.settle_sweeps(sweep_budget as usize);
        self.verify_heap(super::verify::Phase::Minor, roots);
        crate::gc_trace::note_end(
            trace,
            self.young.minor_gc_count,
            self.young.minor_gc_promoted,
            self.young.bytes,
            grown,
            (marked, sweep_started.elapsed()),
        );
    }

    fn seed_edges(&mut self, r: HeapRef) {
        let mut found: Vec<HeapRef> = Vec::new();
        for_each_child(
            &self.cells,
            r,
            &self.identity_index,
            Some(self.young.minor_gc_count as u32),
            &mut |child| {
                if matches!(
                    self.cells.state(child),
                    SlotState::Young | SlotState::Survivor
                ) {
                    found.push(child);
                }
            },
        );
        for child in found {
            self.young.edges.push((r, child));
        }
    }

    fn forget_identity(&mut self, r: HeapRef) {
        if self.instance(r).is_some() {
            return;
        }
        if let Some(key) = Self::identity_key(self.cells.get(r)) {
            self.identity_index.remove(&key);
        }
    }

    fn mark_children(&self, r: HeapRef, work: &mut Vec<HeapRef>) {
        for_each_child(
            &self.cells,
            r,
            &self.identity_index,
            Some(self.young.minor_gc_count as u32),
            &mut |child| CellSpace::mark_young(child, work),
        );
    }

    fn trace_children(&self, r: HeapRef, work: &mut Vec<HeapRef>) -> bool {
        if !CellSpace::take_traced(r) {
            return false;
        }
        let mut young = false;
        for_each_child(
            &self.cells,
            r,
            &self.identity_index,
            Some(self.young.minor_gc_count as u32),
            &mut |child| {
                if self.is_young(VmValue::from_heap(child)) {
                    young = true;
                }
                CellSpace::mark_young(child, work);
            },
        );
        young
    }

    fn sweep_young(&mut self) {
        let mut born = std::mem::take(&mut self.young.born);
        let mut bytes = 0u64;
        let mut survivors = std::mem::take(&mut self.young.survivors);
        let mut next_survivors: Vec<HeapRef> = Vec::with_capacity(survivors.len());
        for &r in &born {
            bytes += self.cells.cell_bytes(r);
            if self.verify_gc && self.cells.state(r) != SlotState::Young {
                panic!(
                    "born cell {r:?} has state {:?} at the start of a minor sweep",
                    self.cells.state(r)
                );
            }
            if CellSpace::is_minor_marked(r) {
                CellSpace::set_state_raw(r, SlotState::Survivor);
                next_survivors.push(r);
                let identity = if self.instance(r).is_some() {
                    None
                } else {
                    Self::identity_key(self.cells.get(r))
                };
                if let Some(key) = identity {
                    self.identity_index.insert(key, r);
                }
            } else {
                self.forget_identity(r);
                self.cells.release(r);
            }
        }
        for &r in &survivors {
            if CellSpace::is_minor_marked(r) {
                self.young.minor_gc_promoted += 1;
                CellSpace::set_state_raw(r, SlotState::Old);
                self.cells.old_bytes +=
                    self.cells.cell_bytes(r) + super::bytes::external_bytes(&self.cells, r);
                if self.remembers_whole(r) {
                    self.young.remembered.push(r);
                } else {
                    self.seed_edges(r);
                }
            } else {
                self.forget_identity(r);
                self.cells.release(r);
            }
        }
        survivors.clear();
        survivors.extend_from_slice(&next_survivors);
        self.young.survivors = survivors;
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
