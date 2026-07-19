use crate::heap::{HeapInner, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;

pub const OLD_GEN_FLAG: u32 = 0x8000_0000;
pub const NURSERY_CAPACITY: usize = 16384;

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
            objects: Vec::with_capacity(4096.min(NURSERY_CAPACITY)),
            forwarding: Vec::with_capacity(4096.min(NURSERY_CAPACITY)),
            remembered: Vec::new(),
            alloc_count: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }

    #[inline(always)]
    pub fn try_alloc(&mut self, obj: HeapObj) -> Result<u32, HeapObj> {
        if self.objects.len() >= NURSERY_CAPACITY {
            return Err(obj);
        }
        let idx = self.objects.len() as u32;
        self.objects.push(Some(obj));
        self.forwarding.push(None);
        self.alloc_count += 1;
        Ok(idx)
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
        self.objects.len() >= Self::FULL_THRESHOLD
    }

    /// Fill level at which [`is_full`] reports true. Exposed so the JIT
    /// back-edge safepoint compares against the same limit.
    pub const FULL_THRESHOLD: usize = NURSERY_CAPACITY * 3 / 4;

    /// Byte offset of the live-object count (`objects.len()`) inside
    /// `Nursery`, for the JIT back-edge safepoint. Relies on Vec's
    /// (cap, ptr, len) word layout — the same assumption the JIT already
    /// makes when it reads `ExecCtx.stack`/`ExecCtx.frames` lengths.
    pub fn objects_len_byte_offset() -> usize {
        std::mem::offset_of!(Nursery, objects) + 2 * std::mem::size_of::<usize>()
    }

    /// Byte offset of the `objects` Vec's three words within `Nursery`,
    /// for the JIT's inline array-read fast path.
    pub fn objects_vec_byte_offset() -> usize {
        std::mem::offset_of!(Nursery, objects)
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Duplicates are allowed here — the write barrier is the hot path, so
    /// dedup happens once per collection instead of O(n) per store.
    #[inline(always)]
    pub fn remember(&mut self, packed_old_idx: u32) {
        self.remembered.push(packed_old_idx);
    }

    pub fn collect(
        &mut self,
        old_gen: &mut HeapInner,
        stack: &mut [VmValue],
        extra_root_packed: &[u32],
    ) {
        self.minor_gc_count += 1;
        let mut worklist: Vec<u32> = Vec::with_capacity(NURSERY_CAPACITY);
        // Reused across every scanned object: one promoted object per scan
        // previously meant one fresh Vec allocation each.
        let mut fixups: Vec<(ChildSlot, u32)> = Vec::with_capacity(8);

        for slot in stack.iter_mut() {
            self.update_value(slot, old_gen, &mut worklist);
        }

        let mut old_indices_to_scan = std::mem::take(&mut self.remembered);
        old_indices_to_scan.sort_unstable();
        old_indices_to_scan.dedup();
        for packed in old_indices_to_scan {
            self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen, &mut worklist, &mut fixups);
        }

        // Closures/classes/modules hold Rust-side `Value`s the write barrier
        // does not cover; scan only the tracked candidates instead of the
        // whole old gen (which made every minor GC O(old-gen size)).
        let candidates: Vec<u32> = old_gen.scan_roots().to_vec();
        for raw_idx in candidates {
            match old_gen.get_raw(raw_idx) {
                Some(HeapObj::VmClosure(_))
                | Some(HeapObj::BoundMethod(_))
                | Some(HeapObj::Class(_))
                | Some(HeapObj::Module(_))
                | Some(HeapObj::Generator(_)) => {
                    self.scan_and_fix_old_obj(raw_idx, old_gen, &mut worklist, &mut fixups);
                }
                _ => {}
            }
        }

        for &packed in extra_root_packed {
            if is_old_idx(packed) {
                self.scan_and_fix_old_obj(old_idx_raw(packed), old_gen, &mut worklist, &mut fixups);
            } else {
                self.evacuate(packed, old_gen, &mut worklist);
            }
        }

        while let Some(raw) = worklist.pop() {
            self.scan_and_fix_old_obj(raw, old_gen, &mut worklist, &mut fixups);
        }

        let n_promoted = self.forwarding.iter().filter(|f| f.is_some()).count();
        self.minor_gc_promoted += n_promoted as u64;
        self.objects.clear();
        self.forwarding.clear();
    }

    #[inline]
    fn update_value(
        &mut self,
        val: &mut VmValue,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
    ) {
        if !val.is_heap() {
            return;
        }
        let idx = val.as_heap_idx();
        if !is_nursery_idx(idx) {
            return;
        }
        let packed = self.evacuate(idx, old_gen, worklist);
        *val = VmValue::from_heap_idx(packed);
    }

    fn evacuate(
        &mut self,
        nursery_idx: u32,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
    ) -> u32 {
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
        worklist.push(raw_old);
        packed
    }

    fn scan_and_fix_old_obj(
        &mut self,
        raw_old: u32,
        old_gen: &mut HeapInner,
        worklist: &mut Vec<u32>,
        fixups: &mut Vec<(ChildSlot, u32)>,
    ) {
        fixups.clear();

        // A generator carries a whole suspended ExecCtx (stack, frames,
        // upvalues, pending suspends) whose slots hold raw heap indices;
        // rewrite them in place through the driver's mutable trace. Clone
        // the Rc first so the borrow of `old_gen` ends before evacuating.
        let gen_driver = match old_gen.get_raw(raw_old) {
            Some(HeapObj::Generator(g)) => Some(g.0.clone()),
            _ => None,
        };
        if let Some(driver) = gen_driver {
            driver.trace_vm_values_mut(&mut |val| {
                self.update_value(val, old_gen, worklist);
            });
            return;
        }

        // Map/Set entries are raw VmValues mutated through interior
        // mutability (the write barrier remembers the collection). Values
        // rewrite in place; canonical keys (interned strings, scalars) are
        // old-gen-stable, but identity keys can move — their hash is their
        // bit pattern, so the table is rebuilt when any key evacuates.
        let map_ref = match old_gen.get_raw(raw_old) {
            Some(HeapObj::Map(m)) => Some(m.clone()),
            _ => None,
        };
        if let Some(m) = map_ref {
            let mut g = m.borrow_mut();
            for v in g.values_mut() {
                self.update_value(v, old_gen, worklist);
            }
            let any_key_moved = g
                .keys()
                .any(|k| k.0.is_heap() && is_nursery_idx(k.0.as_heap_idx()));
            if any_key_moved {
                let entries: Vec<(VmValue, VmValue)> =
                    g.drain().map(|(k, v)| (k.0, v)).collect();
                for (mut k, v) in entries {
                    self.update_value(&mut k, old_gen, worklist);
                    g.insert(varn_types::value::MapKey(k), v);
                }
            }
            return;
        }
        let set_ref = match old_gen.get_raw(raw_old) {
            Some(HeapObj::Set(s)) => Some(s.clone()),
            _ => None,
        };
        if let Some(s) = set_ref {
            let mut g = s.borrow_mut();
            let any_key_moved = g
                .iter()
                .any(|k| k.0.is_heap() && is_nursery_idx(k.0.as_heap_idx()));
            if any_key_moved {
                let items: Vec<VmValue> = g.drain().map(|k| k.0).collect();
                for mut k in items {
                    self.update_value(&mut k, old_gen, worklist);
                    g.insert(varn_types::value::MapKey(k));
                }
            }
            return;
        }

        if let Some(obj) = old_gen.get_raw(raw_old) {
            match obj {
                HeapObj::Array(arr) => {
                    let g = arr.borrow();
                    for (i, &v) in g.iter().enumerate() {
                        if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                            fixups.push((ChildSlot::ArrayItem(i), v.as_heap_idx()));
                        }
                    }
                }
                HeapObj::Object(obj_ref) => {
                    obj_ref.borrow().for_each_field(|i, v| {
                        if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                            fixups.push((ChildSlot::ObjField(i), v.as_heap_idx()));
                        }
                    });
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
                HeapObj::BoundMethod(bm) => {
                    if let Some(idx) = old_gen.value_heap_idx(&bm.receiver) {
                        if is_nursery_idx(idx) {
                            fixups.push((ChildSlot::BoundMethodReceiver, idx));
                        }
                    }
                }
                HeapObj::Class(cls) => {
                    for (i, v) in cls.vtable.borrow().iter().enumerate() {
                        if let Some(idx) = old_gen.value_heap_idx(v) {
                            if is_nursery_idx(idx) {
                                fixups.push((ChildSlot::ClassVtableItem(i), idx));
                            }
                        }
                    }
                    for (k, v) in cls.statics.borrow().iter() {
                        if let Some(idx) = old_gen.value_heap_idx(v) {
                            if is_nursery_idx(idx) {
                                fixups.push((ChildSlot::ClassStatic(k.clone()), idx));
                            }
                        }
                    }
                }
                HeapObj::Module(m) => {
                    for (i, &v) in m.exports.iter().enumerate() {
                        if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                            fixups.push((ChildSlot::ModuleExport(i), v.as_heap_idx()));
                        }
                    }
                }
                HeapObj::EnumVariant(ev) => {
                    if let Some(idx) = old_gen.value_heap_idx(&ev.payload) {
                        if is_nursery_idx(idx) {
                            fixups.push((ChildSlot::EnumVariantPayload, idx));
                        }
                    }
                }
                _ => {}
            }
        }

        for (slot, nursery_idx) in fixups.drain(..) {
            let packed = self.evacuate(nursery_idx, old_gen, worklist);
            let new_val = VmValue::from_heap_idx(packed);
            let extracted_val = if matches!(slot, ChildSlot::BoundMethodReceiver)
                || matches!(slot, ChildSlot::ClassVtableItem(_))
                || matches!(slot, ChildSlot::ClassStatic(_))
                || matches!(slot, ChildSlot::EnumVariantPayload)
            {
                Some(old_gen.extract(new_val))
            } else {
                None
            };
            let Some(obj) = old_gen.get_raw_mut(raw_old) else {
                continue;
            };
            match (slot, obj) {
                (ChildSlot::ArrayItem(i), HeapObj::Array(arr)) => {
                    if let Some(s) = arr.borrow_mut().get_mut(i) {
                        *s = new_val;
                    }
                }
                (ChildSlot::ObjField(i), HeapObj::Object(obj_ref)) => {
                    obj_ref.set_field_at(i, new_val);
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
                (ChildSlot::BoundMethodReceiver, HeapObj::BoundMethod(bm)) => {
                    bm.receiver = extracted_val.unwrap();
                }
                (ChildSlot::ClassVtableItem(i), HeapObj::Class(cls)) => {
                    cls.vtable.borrow_mut()[i] = extracted_val.unwrap();
                }
                (ChildSlot::ClassStatic(k), HeapObj::Class(cls)) => {
                    cls.statics.borrow_mut().insert(k, extracted_val.unwrap());
                }
                (ChildSlot::ModuleExport(i), HeapObj::Module(m)) => {
                    if let Some(s) = Rc::make_mut(m).exports.get_mut(i) {
                        *s = new_val;
                    }
                }
                (ChildSlot::EnumVariantPayload, HeapObj::EnumVariant(ev)) => {
                    ev.payload = extracted_val.unwrap();
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone)]
enum ChildSlot {
    ArrayItem(usize),
    ObjField(usize),
    Upvalue(usize),
    Spread,
    BoundMethodReceiver,
    ClassVtableItem(usize),
    ClassStatic(std::rc::Rc<str>),
    ModuleExport(usize),
    EnumVariantPayload,
}
