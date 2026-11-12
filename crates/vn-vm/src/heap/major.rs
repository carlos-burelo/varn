use super::children::for_each_child;
use super::structs::HeapInner;
use vn_types::HeapRef;

impl HeapInner {
    pub(super) fn mark_and_sweep(&mut self, roots: &[HeapRef]) -> usize {
        let mark_started = std::time::Instant::now();
        self.cells.clear_stale_marks();
        let mut work = std::mem::take(&mut self.major_work);
        work.clear();
        for &root in roots {
            if self.cells.mark_major(root) {
                work.push(root);
            }
        }
        while let Some(r) = work.pop() {
            for_each_child(&self.cells, r, &self.identity_index, None, &mut |child| {
                if self.cells.mark_major(child) {
                    work.push(child);
                }
            });
        }
        self.major_work = work;
        let marked = mark_started.elapsed();
        self.compact_interners();
        let freed = self.cells.begin_major();
        crate::gc_trace::major_phases(marked, std::time::Duration::ZERO);
        freed
    }
}
