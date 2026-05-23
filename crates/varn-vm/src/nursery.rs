/// Nursery (young generation) bump allocator.
///
/// Only `Str`, `Array`, and `Object` objects are nursery-eligible — they
/// account for the vast majority of short-lived allocations.  All other
/// types (VmClosure, Class, Task, Generator, …) are allocated directly in
/// the old generation, avoiding the complexity of evacuating them.
///
/// ## Index encoding
///
/// VmValue stores a raw u32 heap index in its low 32 bits (TAG_PTR).
/// We steal bit 31 to distinguish generations:
///
///   bit31 = 0 → nursery index  (0 .. NURSERY_CAPACITY-1)
///   bit31 = 1 → old-gen index  (raw_old_idx | OLD_GEN_FLAG)
///
/// `as_heap_idx()` returns this raw u32 unchanged — all existing VM code
/// that reads object slots already routes through `heap.get()` /
/// `nursery.get()` helpers and therefore never needs to strip the flag
/// itself.  Only the allocator and GC inspect the flag bit.
use crate::heap::{HeapInner, HeapObj};
use crate::value::VmValue;

/// Bit 31 marks an old-gen index packed into a VmValue.
pub const OLD_GEN_FLAG: u32 = 0x8000_0000;
/// Maximum number of live objects the nursery can hold before Minor GC.
pub const NURSERY_CAPACITY: usize = 2048;

#[inline(always)]
pub fn is_nursery_idx(idx: u32) -> bool {
    (idx & OLD_GEN_FLAG) == 0
}

#[inline(always)]
pub fn is_old_idx(idx: u32) -> bool {
    (idx & OLD_GEN_FLAG) != 0
}

/// Strip the OLD_GEN_FLAG from a packed old-gen index.
#[inline(always)]
pub fn old_idx_raw(packed: u32) -> u32 {
    packed & !OLD_GEN_FLAG
}

/// Add OLD_GEN_FLAG to a raw old-gen index so it can be stored in VmValue.
#[inline(always)]
pub fn pack_old_idx(raw_old: u32) -> u32 {
    raw_old | OLD_GEN_FLAG
}

#[derive(Clone)]
pub struct Nursery {
    objects: Vec<Option<HeapObj>>,
    /// forwarding[nursery_idx] = Some(packed_old_idx) after evacuation.
    forwarding: Vec<Option<u32>>,
    /// Remembered set: packed old-gen indices whose object contains at least
    /// one nursery pointer.  Used as extra roots during Minor GC.
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

    /// Bump-allocate. Returns the nursery index (bit31 = 0) or `None` if full.
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

    /// Look up a nursery object by its raw nursery index.
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

    /// Record that the old-gen object at `packed_old_idx` holds a nursery ref.
    #[inline(always)]
    pub fn remember(&mut self, packed_old_idx: u32) {
        // Linear scan is acceptable: remembered set is tiny in practice.
        if !self.remembered.contains(&packed_old_idx) {
            self.remembered.push(packed_old_idx);
        }
    }

    // -----------------------------------------------------------------------
    // Minor GC
    // -----------------------------------------------------------------------

    /// Evacuate all live nursery objects into `old_gen`.
    ///
    /// `stack` is mutated in-place: every nursery pointer in `stack[..]` is
    /// updated to its new old-gen packed index.
    ///
    /// `extra_root_packed` is a slice of packed old-gen indices for objects
    /// (globals, module cache, frame constants) that may contain nursery refs.
    pub fn collect(
        &mut self,
        old_gen: &mut HeapInner,
        stack: &mut [VmValue],
        extra_root_packed: &[u32],
    ) {
        self.minor_gc_count += 1;

        // Phase 1 — evacuate from stack roots.
        for slot in stack.iter_mut() {
            self.update_value(slot, old_gen);
        }

        // Phase 2 — scan only the remembered old-gen objects for nursery pointers.
        // This is the optimized generational approach enabled by our write barrier.
        // Cost is proportional only to the number of modified old-gen objects.
        let old_indices_to_scan = std::mem::take(&mut self.remembered);
        for packed in old_indices_to_scan {
            self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen);
        }

        // Also scan all live VmClosures in the heap, since they can contain closed upvalues
        // referencing nursery objects but are not tracked by the write barrier.
        for raw_idx in 0..old_gen.objects().len() {
            if let Some(HeapObj::VmClosure(_)) = old_gen.get_raw(raw_idx as u32) {
                self.scan_and_fix_old_obj(raw_idx as u32, old_gen);
            }
        }

        // Phase 3 — evacuate from extra roots (globals, frame constants, modules)
        // that may themselves be nursery values or old-gen objects with nursery refs.
        for &packed in extra_root_packed {
            if is_old_idx(packed) {
                self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen);
            } else {
                self.evacuate(packed, old_gen);
            }
        }

        // Phase 4 — fix pointers inside newly promoted objects.
        // Promoted objects may reference other nursery objects evacuated
        // in later steps; we do a second pass to close the gap.
        let promoted_raw: Vec<u32> = self
            .forwarding
            .iter()
            .filter_map(|f| f.map(old_idx_raw))
            .collect();
        for raw in promoted_raw {
            self.scan_and_fix_old_obj(raw, old_gen);
        }

        // Phase 5 — reset nursery.
        let n_promoted = self.forwarding.iter().filter(|f| f.is_some()).count();
        self.minor_gc_promoted += n_promoted as u64;
        self.objects.clear();
        self.forwarding.clear();
        // `remembered` was already moved out above; it's now empty.
    }

    /// If `val` is a nursery pointer, evacuate the object and update `val`.
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

    /// Evacuate nursery object at `nursery_idx` → old gen. Returns packed idx.
    /// Idempotent: returns the existing forwarding entry if already evacuated.
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

    /// Scan an old-gen object and fix any nursery VmValue references it contains.
    fn scan_and_fix_old_obj(&mut self, raw_old: u32, old_gen: &mut HeapInner) {
        // We must avoid holding a borrow on `old_gen` while also calling
        // `self.evacuate(old_gen)`.  Strategy: collect nursery child indices
        // into a small stack-allocated vec, then fix each one.
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

        // Now apply fixups — we have mutable access to old_gen again.
        for (slot, nursery_idx) in fixups {
            let packed = self.evacuate(nursery_idx, old_gen);
            let new_val = VmValue::from_heap_idx(packed);
            let Some(obj) = old_gen.get_raw_mut(raw_old) else { continue };
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
