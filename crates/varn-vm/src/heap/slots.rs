//! The one slot table every heap handle indexes, and the generation of each
//! slot. Objects never move: promotion rewrites a state byte, never a handle,
//! so nothing outside the collector ever has to be told an index changed.

use super::obj::HeapObj;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SlotState {
    Old = 0,
    Remembered = 1,
    Young = 2,
    Marked = 3,
}

pub(crate) struct SlotTable {
    objects: Vec<Option<HeapObj>>,
    states: Vec<SlotState>,
    free: Vec<u32>,
    pub(crate) births: u64,
    pub(crate) old_growth: u64,
}

impl SlotTable {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            objects: Vec::with_capacity(capacity),
            states: Vec::with_capacity(capacity),
            free: Vec::new(),
            births: 0,
            old_growth: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn alloc(&mut self, obj: HeapObj, state: SlotState) -> u32 {
        self.births += 1;
        if state == SlotState::Old {
            self.old_growth += 1;
        }
        match self.free.pop() {
            Some(idx) => {
                self.objects[idx as usize] = Some(obj);
                self.states[idx as usize] = state;
                idx
            }
            None => {
                let idx = self.objects.len() as u32;
                self.objects.push(Some(obj));
                self.states.push(state);
                idx
            }
        }
    }

    #[inline(always)]
    pub(crate) fn get(&self, idx: u32) -> Option<&HeapObj> {
        self.objects.get(idx as usize)?.as_ref()
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        self.objects.get_mut(idx as usize)?.as_mut()
    }

    #[inline(always)]
    pub(crate) fn state(&self, idx: u32) -> SlotState {
        self.states
            .get(idx as usize)
            .copied()
            .unwrap_or(SlotState::Old)
    }

    #[inline(always)]
    pub(crate) fn set_state(&mut self, idx: u32, state: SlotState) {
        self.states[idx as usize] = state;
    }

    /// The object in `idx` together with every slot's state, so a collector
    /// can walk one object's children and mark them in the same pass.
    #[inline]
    pub(crate) fn split_at(&mut self, idx: u32) -> (Option<&HeapObj>, &mut [SlotState]) {
        let obj = self.objects.get(idx as usize).and_then(|o| o.as_ref());
        (obj, &mut self.states)
    }

    pub(crate) fn promote(&mut self, idx: u32) {
        self.states[idx as usize] = SlotState::Old;
        self.old_growth += 1;
    }

    /// Drops the object where it stands: moving it out first would copy the
    /// whole slot for every dead object a sweep frees.
    #[inline(always)]
    pub(crate) fn release(&mut self, idx: u32) {
        self.objects[idx as usize] = None;
        self.free.push(idx);
    }

    pub(crate) fn is_live(&self, idx: u32) -> bool {
        self.get(idx).is_some()
    }

    pub(crate) fn len(&self) -> u32 {
        self.objects.len() as u32
    }

    pub(crate) fn free_len(&self) -> usize {
        self.free.len()
    }

    pub(crate) fn live_count(&self) -> usize {
        self.objects.len() - self.free.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, &HeapObj, SlotState)> {
        self.objects
            .iter()
            .zip(&self.states)
            .enumerate()
            .filter_map(|(i, (o, s))| o.as_ref().map(|o| (i as u32, o, *s)))
    }

    pub(crate) fn objects_vec_byte_offset() -> usize {
        std::mem::offset_of!(SlotTable, objects)
    }

    pub(crate) fn states_vec_byte_offset() -> usize {
        std::mem::offset_of!(SlotTable, states)
    }
}
