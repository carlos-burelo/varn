//! Major collection: mark everything reachable from the roots across every
//! cell, young and old alike, then free every unmarked cell and leave every
//! survivor old.

use super::cells::CellSpace;
use super::children::{for_each_child, Reach};
use super::structs::HeapInner;
use varn_types::HeapRef;

impl HeapInner {
    pub(super) fn mark_and_sweep(&mut self, roots: &[HeapRef]) -> usize {
        let mut work = std::mem::take(&mut self.major_work);
        work.clear();
        for &root in roots {
            if CellSpace::mark_major(root) {
                work.push(root);
            }
        }
        while let Some(r) = work.pop() {
            for_each_child(
                self.cells.get(r),
                &self.identity_index,
                Reach::Major,
                &mut |child| {
                    if CellSpace::mark_major(child) {
                        work.push(child);
                    }
                },
            );
        }
        self.major_work = work;
        self.cells.sweep_major()
    }
}
