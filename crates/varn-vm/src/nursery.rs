use crate::heap::{HeapInner, HeapObj};
use crate::value::VmValue;

pub const OLD_GEN_FLAG: u32 = 0x8000_0000;
pub const NURSERY_CAPACITY: usize = 2048;

#[inline(always)]
pub fn is_nursery_idx(idx: u32) -> bool {
    (idx & OLD_GEN_FLAG) == 0
}

#[inline(always)]
pub fn is_old_idx(idx: u32) -> bool {
    (idx & OLD_GEN_FLAG) != 0
}

#[inline(always)]
pub fn old_idx_raw(packed: u32) -> u32 {
    packed & !OLD_GEN_FLAG
}

#[inline(always)]
pub fn pack_old_idx(raw_old: u32) -> u32 {
    raw_old | OLD_GEN_FLAG
}

#[derive(Clone)]
pub struct Nursery {
    objects: Vec<Option<HeapObj>>,
    forwarding: Vec<Option<u32>>,
    pub remembered: Vec<u32>,
    pub alloc_count: u64,
    pub minor_gc_count: u64,
    pub minor_gc_promoted: u64,
}

impl Default for Nursery {
    fn default() -> Self {
        Self::new()
    }
}

impl Nursery {
    pub fn new() -> Self {
        Self {
            objects: Vec::with_capacity(NURSERY_CAPACITY),
            forwarding: Vec::with_capacity(NURSERY_CAPACITY),
            remembered: Vec::new(),
            alloc_count: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }

    #[inline(always)]
    pub fn try_alloc(&mut self, obj: HeapObj) -> Option<u32> {
        if self.objects.len() >= NURSERY_CAPACITY {
            return None;
        }
        let idx = self.objects.len() as u32;
        self.objects.push(Some(obj));
        self.forwarding.push(None);
        self.alloc_count += 1;
        Some(idx)
    }

    #[inline(always)]
    pub fn get(&self, idx: u32) -> Option<&HeapObj> {
        self.objects.get(idx as usize)?.as_ref()
    }

    #[inline(always)]
    pub fn get_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        self.objects.get_mut(idx as usize)?.as_mut()
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.objects.len() >= NURSERY_CAPACITY
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    #[inline(always)]
    pub fn remember(&mut self, packed_old_idx: u32) {
        if !self.remembered.contains(&packed_old_idx) {
            self.remembered.push(packed_old_idx);
        }
    }

    pub fn collect(
        &mut self,
        old_gen: &mut HeapInner,
        stack: &mut [VmValue],
        extra_root_packed: &[u32],
    ) {
        self.minor_gc_count += 1;

        for slot in stack.iter_mut() {
            self.update_value(slot, old_gen);
        }

        let old_indices_to_scan = std::mem::take(&mut self.remembered);
        for packed in old_indices_to_scan {
            self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen);
        }

        for raw_idx in 0..old_gen.objects().len() {
            if let Some(HeapObj::VmClosure(_)) = old_gen.get_raw(raw_idx as u32) {
                self.scan_and_fix_old_obj(raw_idx as u32, old_gen);
            }
        }

        for &packed in extra_root_packed {
            if is_old_idx(packed) {
                self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen);
            } else {
                self.evacuate(packed, old_gen);
            }
        }

        let promoted_raw: Vec<u32> = self
            .forwarding
            .iter()
            .filter_map(|f| f.map(old_idx_raw))
            .collect();
        for raw in promoted_raw {
            self.scan_and_fix_old_obj(raw, old_gen);
        }

        let n_promoted = self.forwarding.iter().filter(|f| f.is_some()).count();
        self.minor_gc_promoted += n_promoted as u64;
        self.objects.clear();
        self.forwarding.clear();
    }

    #[inline]
    fn update_value(&mut self, val: &mut VmValue, old_gen: &mut HeapInner) {
        if !val.is_heap() {
            return;
        }
        let idx = val.as_heap_idx();
        if !is_nursery_idx(idx) {
            return;
        }
        let packed = self.evacuate(idx, old_gen);
        *val = VmValue::from_heap_idx(packed);
    }

    fn evacuate(&mut self, nursery_idx: u32, old_gen: &mut HeapInner) -> u32 {
        if let Some(Some(fwd)) = self.forwarding.get(nursery_idx as usize) {
            return *fwd;
        }
        let obj = match self.objects.get_mut(nursery_idx as usize) {
            Some(slot @ &mut Some(_)) => slot.take().unwrap(),
            _ => return pack_old_idx(0),
        };
        let raw_old = old_gen.alloc_raw(obj);
        let packed = pack_old_idx(raw_old);
        if let Some(slot) = self.forwarding.get_mut(nursery_idx as usize) {
            *slot = Some(packed);
        }
        packed
    }

    fn scan_and_fix_old_obj(&mut self, raw_old: u32, old_gen: &mut HeapInner) {
        let mut fixups: Vec<(ChildSlot, u32)> = Vec::with_capacity(8);

        if let Some(obj) = old_gen.get_raw(raw_old) {
            match obj {
                HeapObj::Array(arr) => {
                    let g = arr.0.borrow();
                    for (i, &v) in g.iter().enumerate() {
                        if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                            fixups.push((ChildSlot::ArrayItem(i), v.as_heap_idx()));
                        }
                    }
                }
                HeapObj::Object(obj_ref) => {
                    let g = obj_ref.borrow();
                    for (i, &v) in g.inner.values.iter().enumerate() {
                        if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                            fixups.push((ChildSlot::ObjField(i), v.as_heap_idx()));
                        }
                    }
                }
                HeapObj::VmClosure(clos) => {
                    for (i, uv) in clos.upvalues.iter().enumerate() {
                        if let Ok(inner) = uv.inner.try_borrow() {
                            if inner.value.is_heap() && is_nursery_idx(inner.value.as_heap_idx()) {
                                fixups.push((ChildSlot::Upvalue(i), inner.value.as_heap_idx()));
                            }
                        }
                    }
                }
                HeapObj::Spread(v) => {
                    if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                        fixups.push((ChildSlot::Spread, v.as_heap_idx()));
                    }
                }
                _ => {}
            }
        }

        for (slot, nursery_idx) in fixups {
            let packed = self.evacuate(nursery_idx, old_gen);
            let new_val = VmValue::from_heap_idx(packed);
            let Some(obj) = old_gen.get_raw_mut(raw_old) else {
                continue;
            };
            match (slot, obj) {
                (ChildSlot::ArrayItem(i), HeapObj::Array(arr)) => {
                    if let Some(s) = arr.0.borrow_mut().get_mut(i) {
                        *s = new_val;
                    }
                }
                (ChildSlot::ObjField(i), HeapObj::Object(obj_ref)) => {
                    if let Some(s) = obj_ref.borrow_mut().inner.values.get_mut(i) {
                        *s = new_val;
                    }
                }
                (ChildSlot::Upvalue(i), HeapObj::VmClosure(clos)) => {
                    if let Some(uv) = clos.upvalues.get(i) {
                        if let Ok(mut inner) = uv.inner.try_borrow_mut() {
                            inner.value = new_val;
                        }
                    }
                }
                (ChildSlot::Spread, HeapObj::Spread(v)) => {
                    *v = new_val;
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy)]
enum ChildSlot {
    ArrayItem(usize),
    ObjField(usize),
    Upvalue(usize),
    Spread,
}
