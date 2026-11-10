use super::*;

impl CellSpace {
    #[inline(always)]
    pub(crate) fn mark_young(r: HeapRef, work: &mut Vec<HeapRef>) {
        let h = header(r);
        if h.state & MINOR_MARK != 0 {
            return;
        }
        let state = h.state & STATE_MASK;
        if state == SlotState::Young as u8 || state == SlotState::Survivor as u8 {
            h.state |= MINOR_MARK;
            if h.kind == varn_types::cell::CELL_KIND_INSTANCE && !instance_has_references(r) {
                return;
            }
            work.push(r);
        }
    }

    pub(crate) fn mark_major(&self, r: HeapRef) -> bool {
        let h = header(r);
        let state = h.state & STATE_MASK;
        if state == SlotState::Free as u8 {
            return false;
        }
        let class = h.class;
        let fresh = if class == CELL_CLASS_LARGE {
            if h.state & MAJOR_MARK != 0 {
                return false;
            }
            h.state |= MAJOR_MARK;
            true
        } else {
            let block = block_of(r.addr());
            let b = block_ref_mut(block);
            let cell = CELL_CLASSES[class as usize];
            let index = ((r.addr() - b.cells_start()) / cell as u64) as usize;
            b.mark(index)
        };
        if fresh && state != SlotState::Old as u8 {
            set_age(r, SlotState::Old);
        }
        fresh && (h.kind != varn_types::cell::CELL_KIND_INSTANCE || instance_has_references(r))
    }

    #[inline(always)]
    pub(crate) fn is_marked(&self, r: HeapRef) -> bool {
        let h = header(r);
        if h.class == CELL_CLASS_LARGE {
            return h.state & MAJOR_MARK != 0;
        }
        let b = block_ref(block_of(r.addr()));
        let cell = CELL_CLASSES[h.class as usize];
        let index = ((r.addr() - b.cells_start()) / cell as u64) as usize;
        b.is_marked(index)
    }

    #[inline(always)]
    pub(crate) fn is_minor_marked(r: HeapRef) -> bool {
        header(r).state & MINOR_MARK != 0
    }

    pub(crate) fn will_die(&self, r: HeapRef) -> bool {
        matches!(self.state(r), SlotState::Old | SlotState::Remembered) && !self.is_marked(r)
    }

    #[inline(always)]
    pub(crate) fn set_state_raw(r: HeapRef, state: SlotState) {
        set_age(r, state);
    }

    #[inline(always)]
    pub(crate) fn take_traced(r: HeapRef) -> bool {
        let h = header(r);
        if h.state & TRACED != 0 {
            return false;
        }
        h.state |= TRACED;
        true
    }

    #[inline(always)]
    pub(crate) fn clear_traced(r: HeapRef) {
        header(r).state &= !TRACED;
    }

    #[inline(always)]
    pub(crate) fn raw_state(r: HeapRef) -> u8 {
        header(r).state
    }
}
