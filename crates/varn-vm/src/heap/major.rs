//! Major collection: mark everything reachable from the roots across the
//! whole slot table, young and old alike, then free every unmarked slot and
//! leave every survivor old.

use super::children::{for_each_child, Reach};
use super::slots::SlotState;
use super::structs::HeapInner;

#[derive(Default)]
pub(crate) struct MajorMarks {
    bits: Vec<u64>,
    work: Vec<u32>,
}

impl MajorMarks {
    #[inline]
    fn mark(bits: &mut [u64], work: &mut Vec<u32>, idx: u32) {
        let (word, bit) = (idx as usize / 64, 1u64 << (idx % 64));
        if let Some(w) = bits.get_mut(word) {
            if *w & bit == 0 {
                *w |= bit;
                work.push(idx);
            }
        }
    }

    #[inline]
    fn is_marked(&self, idx: u32) -> bool {
        self.bits[idx as usize / 64] & (1u64 << (idx % 64)) != 0
    }
}

impl HeapInner {
    pub(super) fn mark_and_sweep(&mut self, roots: &[u32]) -> usize {
        let mut m = std::mem::take(&mut self.major);
        let len = self.slots.len();
        m.bits.clear();
        m.bits.resize((len as usize).div_ceil(64), 0);
        m.work.clear();

        for &root in roots {
            if self.slots.is_live(root) {
                MajorMarks::mark(&mut m.bits, &mut m.work, root);
            }
        }
        while let Some(idx) = m.work.pop() {
            if let Some(obj) = self.slots.get(idx) {
                let (bits, work) = (&mut m.bits, &mut m.work);
                for_each_child(obj, &self.identity_index, Reach::Major, &mut |child| {
                    MajorMarks::mark(bits, work, child)
                });
            }
        }

        let mut freed = 0;
        for idx in 0..len {
            if !self.slots.is_live(idx) {
                continue;
            }
            if m.is_marked(idx) {
                self.slots.set_state(idx, SlotState::Old);
            } else {
                self.slots.release(idx);
                freed += 1;
            }
        }
        self.major = m;
        freed
    }
}
