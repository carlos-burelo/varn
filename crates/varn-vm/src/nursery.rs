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

impl Clone for Nursery {
    /// Hand-written, not derived. A derived `Clone` would clone `objects`
    /// and `forwarding` via `Vec::clone` (`slice::to_vec`), which allocates
    /// with `capacity == len` — silently dropping the "capacity is
    /// `NURSERY_CAPACITY` from birth and never changes" invariant `new`
    /// establishes and `emit_nursery_alloc` depends on for its raw slot
    /// address to stay valid.
    ///
    /// This is reachable, not theoretical: `Heap::deep_clone` clones
    /// `HeapInner`, which clones `Nursery`; `deep_clone`'s only caller is
    /// `Vm::from_snapshot`, whose only caller is the bench harness
    /// (`crates/varn-cli/src/bench/harness.rs`) — every single `vn bench`
    /// iteration deep-clones a fresh heap this way. Without this override,
    /// every VM `vn bench` builds would run on a nursery whose backing
    /// store can move at the first `push` that exceeds its (small,
    /// length-sized) cloned capacity — silent corruption the moment a
    /// caller (Task 7) holds a raw slot address across such a `push`.
    ///
    /// Reserves exactly like `new`, then copies contents in — same
    /// allocation cost `new` already pays, paid again here rather than
    /// left cheaper-but-unsound.
    fn clone(&self) -> Self {
        let mut objects = Vec::with_capacity(NURSERY_CAPACITY);
        objects.extend(self.objects.iter().cloned());
        let mut forwarding = Vec::with_capacity(NURSERY_CAPACITY);
        forwarding.extend(self.forwarding.iter().cloned());
        Self {
            objects,
            forwarding,
            remembered: self.remembered.clone(),
            alloc_count: self.alloc_count,
            minor_gc_count: self.minor_gc_count,
            minor_gc_promoted: self.minor_gc_promoted,
        }
    }
}

impl Nursery {
    pub fn new() -> Self {
        // Full capacity from birth, not grown into. `try_alloc` pushes to
        // `objects` and `forwarding` together and the minor collector indexes
        // both by nursery index, so a realloc in either is a moving backing
        // store — which a planned JIT inline bump and a planned
        // `Heap::alloc_str_concat_inline` (JIT string-codegen plan, Tasks 2
        // and 5; neither exists yet) will need to assume cannot happen. Fixed
        // size, so this is ~900 KB paid once rather than a growth curve —
        // for the nursery a live `HeapInner` owns. See `vacant` for the
        // placeholder used when a `Nursery` is briefly swapped out, which
        // must NOT pay this cost.
        Self {
            objects: Vec::with_capacity(NURSERY_CAPACITY),
            forwarding: Vec::with_capacity(NURSERY_CAPACITY),
            remembered: Vec::new(),
            alloc_count: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }

    /// An empty, non-allocating placeholder — capacity 0 in both `objects`
    /// and `forwarding`, so construction touches no allocator.
    ///
    /// For use only as a swap target while the real nursery is moved out
    /// (see `HeapInner::minor_gc`). `Default`/`new` reserve
    /// `NURSERY_CAPACITY` up front (~900 KB) so that a *live* nursery never
    /// reallocates; `mem::take`, which builds a `Default`, would pay that
    /// same ~900 KB on every single minor GC to build a value that is
    /// dropped a few lines later. `Vec::new()` is documented not to
    /// allocate until pushed to, so this constructor is zero-cost; see
    /// `capacity_invariant::vacant_nursery_allocates_nothing`.
    pub fn vacant() -> Self {
        Self {
            objects: Vec::new(),
            forwarding: Vec::new(),
            remembered: Vec::new(),
            alloc_count: 0,
            minor_gc_count: 0,
            minor_gc_promoted: 0,
        }
    }

    /// Capacity of the object slots. Constant for the nursery's lifetime —
    /// see `new`. Exposed so the invariant test and the JIT's emitted bounds
    /// check assert against one number.
    pub fn objects_capacity(&self) -> usize {
        self.objects.capacity()
    }

    /// Capacity of the forwarding slots, which must track `objects`.
    pub fn forwarding_capacity(&self) -> usize {
        self.forwarding.capacity()
    }

    /// Backing-store address of the object slots, for the invariant test.
    pub fn objects_data_ptr(&self) -> *const Option<HeapObj> {
        self.objects.as_ptr()
    }

    /// Backing-store address of the forwarding slots, for the invariant test
    /// and for `ExecCtx::validate_jit_safepoint_offsets` — the one raw
    /// offset `emit_nursery_alloc`'s `None`-write uses
    /// (`nursery_fwd_vec_off + slots_ptr_off`) that nothing else in the
    /// write path independently re-derives, since `slots_ptr_off` is probed
    /// against `Vec<Option<HeapObj>>` and reused here for `Vec<Option<u32>>`.
    pub fn forwarding_data_ptr(&self) -> *const Option<u32> {
        self.forwarding.as_ptr()
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

    /// Byte offset of the `forwarding` Vec's three words within `Nursery`,
    /// for the JIT's inline allocation — which must bump both Vecs, since the
    /// minor collector indexes them together.
    pub fn forwarding_vec_byte_offset() -> usize {
        std::mem::offset_of!(Nursery, forwarding)
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

        // ONE lookup, whatever the variant. This runs once per promoted
        // object, and probing `get_raw` separately for Generator, then Map,
        // then Set, then everything else charged four bounds-checked slot
        // reads to reach the common case (a plain Object or Array). The
        // three container variants still have to hand their handle out of
        // the borrow — they evacuate THROUGH `old_gen`, so its borrow must
        // end first — but every other variant is scanned right here.
        let deferred = match old_gen.get_raw(raw_old) {
            Some(HeapObj::Generator(g)) => Container::Generator(g.0.clone()),
            Some(HeapObj::Map(m)) => Container::Map(m.clone()),
            Some(HeapObj::Set(s)) => Container::Set(s.clone()),
            Some(obj) => {
                Self::scan_children(obj, old_gen, fixups);
                Container::None
            }
            None => Container::None,
        };

        match deferred {
            // A generator carries a whole suspended ExecCtx (stack, frames,
            // upvalues, pending suspends) whose slots hold raw heap indices;
            // rewrite them in place through the driver's mutable trace.
            Container::Generator(driver) => {
                driver.trace_vm_values_mut(&mut |val| {
                    self.update_value(val, old_gen, worklist);
                });
                return;
            }
            // Map/Set entries are raw VmValues mutated through interior
            // mutability (the write barrier remembers the collection).
            // Values rewrite in place; canonical keys (interned strings,
            // scalars) are old-gen-stable, but identity keys can move —
            // their hash is their bit pattern, so the table is rebuilt when
            // any key evacuates.
            Container::Map(m) => {
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
            Container::Set(s) => {
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
            Container::None => {}
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
                    // `ArrayItem` fixups are only ever pushed for a `Boxed`
                    // array (see the scan above), but use the total accessor
                    // rather than `borrow_mut()` anyway — never assume a
                    // repr invariant here that isn't locally re-checked.
                    if let Some(v) = arr.as_boxed_mut() {
                        if let Some(s) = v.get_mut(i) {
                            *s = new_val;
                        }
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

    /// Record every child of `obj` that still points into the nursery. Pure
    /// scan: it only reads through the borrow of `old_gen` the caller holds,
    /// so the evacuation that consumes `fixups` happens after it returns.
    fn scan_children(
        obj: &HeapObj,
        old_gen: &HeapInner,
        fixups: &mut Vec<(ChildSlot, u32)>,
    ) {
        match obj {
            HeapObj::Array(arr) => {
                // Variant-aware: I64/F64 elements are raw numeric words,
                // never a nursery heap ref, so there is nothing to scan
                // or fix up. Only `Boxed` can hold a moved child.
                if let Some(items) = arr.as_boxed() {
                    for (i, &v) in items.iter().enumerate() {
                        if v.is_heap() && is_nursery_idx(v.as_heap_idx()) {
                            fixups.push((ChildSlot::ArrayItem(i), v.as_heap_idx()));
                        }
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
}

#[cfg(test)]
mod array_scan_tests {
    use super::*;
    use varn_types::VmArray;

    /// A.2: `scan_and_fix_old_obj`'s `HeapObj::Array` arm must branch on the
    /// repr (`as_boxed()`), never call the Boxed-only `borrow()` — that
    /// panics on a typed repr. I64/F64 elements are raw numeric words, never
    /// a nursery heap ref, so scanning either must not panic and must not
    /// evacuate anything (nothing pushed onto `worklist`).
    #[test]
    fn scan_skips_typed_array_reprs_without_panicking() {
        let mut old_gen = HeapInner::new();
        let raw_i64 = old_gen.alloc_raw(HeapObj::Array(VmArray::new_i64(vec![1, 2, 3])));
        let raw_f64 = old_gen.alloc_raw(HeapObj::Array(VmArray::new_f64(vec![1.0, 2.0])));

        let mut nursery = Nursery::new();
        let mut worklist = Vec::new();
        let mut fixups = Vec::new();

        nursery.scan_and_fix_old_obj(raw_i64, &mut old_gen, &mut worklist, &mut fixups);
        nursery.scan_and_fix_old_obj(raw_f64, &mut old_gen, &mut worklist, &mut fixups);

        assert!(worklist.is_empty());
    }

    /// Control case: a `Boxed` array element pointing into the nursery is
    /// still evacuated and the array slot rewritten in place — the
    /// variant-aware rewrite must not regress the path every existing test
    /// in the suite exercises.
    #[test]
    fn scan_still_evacuates_and_fixes_up_boxed_array_nursery_refs() {
        let mut old_gen = HeapInner::new();
        let mut nursery = Nursery::new();

        let child_nursery_idx = nursery
            .try_alloc(HeapObj::Str(crate::heap::HeapStr::shared(std::rc::Rc::from(
                "child",
            ))))
            .expect("fresh nursery has room");
        let child_val = VmValue::from_heap_idx(child_nursery_idx);
        assert!(is_nursery_idx(child_val.as_heap_idx()));

        let raw_arr = old_gen.alloc_raw(HeapObj::Array(VmArray::new(vec![child_val])));

        let mut worklist = Vec::new();
        let mut fixups = Vec::new();
        nursery.scan_and_fix_old_obj(raw_arr, &mut old_gen, &mut worklist, &mut fixups);

        // The nursery child was evacuated into the old gen...
        assert_eq!(worklist.len(), 1);
        // ...and the fixup loop rewrote the array slot in place to point at
        // its new old-gen (packed) index.
        match old_gen.get_raw(raw_arr) {
            Some(HeapObj::Array(arr)) => {
                let items = arr.as_boxed().expect("array stays Boxed");
                assert_eq!(items.len(), 1);
                assert!(items[0].is_heap());
                assert!(is_old_idx(items[0].as_heap_idx()));
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }
}

/// The variants `scan_and_fix_old_obj` cannot scan under the borrow that
/// found them: each rewrites its contents by evacuating THROUGH `old_gen`,
/// so the handle has to leave the borrow first. Everything else is scanned
/// in place by `scan_children` and reports `None` here.
enum Container {
    Generator(Rc<dyn varn_types::generator::GeneratorDriver>),
    Map(varn_types::value::MapRef),
    Set(varn_types::value::SetRef),
    None,
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

#[cfg(test)]
mod capacity_invariant {
    use super::*;
    use crate::heap::{HeapObj, HeapStr};

    /// Stage B emits a nursery bump inline, which is only sound if a `push`
    /// can never move the backing store. That makes "capacity is
    /// NURSERY_CAPACITY from birth and never changes" a load-bearing
    /// invariant rather than a tuning detail.
    #[test]
    fn nursery_capacity_is_fixed_from_birth() {
        let mut n = Nursery::new();
        assert_eq!(n.objects_capacity(), NURSERY_CAPACITY);
        assert_eq!(n.forwarding_capacity(), NURSERY_CAPACITY);

        let objects_ptr = n.objects_data_ptr();
        for i in 0..NURSERY_CAPACITY {
            let obj = HeapObj::Str(HeapStr::inline("x"));
            assert!(n.try_alloc(obj).is_ok(), "alloc {i} should fit");
        }
        assert_eq!(n.objects_capacity(), NURSERY_CAPACITY, "capacity grew");
        assert_eq!(
            n.objects_data_ptr(),
            objects_ptr,
            "backing store moved — an inline bump would write into freed memory"
        );

        // One past capacity must decline, not grow.
        assert!(n.try_alloc(HeapObj::Str(HeapStr::inline("y"))).is_err());
        assert_eq!(n.objects_capacity(), NURSERY_CAPACITY);
    }

    /// A collection must not hand back a smaller (or relocated) nursery.
    ///
    /// The brief's scaffolding named this entry point `collect_minor`; the
    /// actual API (see `heap.rs`) calls it `HeapInner::minor_gc`. It
    /// `mem::replace`s the nursery with `Nursery::vacant()`, calls
    /// `Nursery::collect` on the owned (real) value, then writes it back — a
    /// struct move, which does not touch the Vecs' heap-allocated backing
    /// stores, so the pointer assertion below still holds.
    #[test]
    fn nursery_capacity_survives_a_collection() {
        let heap = crate::heap::Heap::new();
        let mut stack: Vec<VmValue> = Vec::new();
        for i in 0..1000 {
            let v = unsafe { heap.inner_mut() }
                .alloc_str_dynamic(format!("keep-{i}-padding-to-exceed-sso"));
            stack.push(v);
        }
        let inner = unsafe { heap.inner_mut() };
        let before = inner.nursery.objects_data_ptr();
        inner.minor_gc(&mut stack, &[]);
        assert_eq!(inner.nursery.objects_capacity(), NURSERY_CAPACITY);
        assert_eq!(
            inner.nursery.objects_data_ptr(),
            before,
            "collection relocated the nursery backing store"
        );
    }

    /// `HeapInner::minor_gc` swaps a `Nursery::vacant()` into `self.nursery`
    /// for the duration of `collect`, once per minor GC. If `vacant` ever
    /// regressed to reserving real capacity (e.g. someone "simplifying" it
    /// back to `Self::new()`), every minor GC would pay a ~900 KB
    /// malloc+free for a value nothing reads. Capacity 0 is the direct,
    /// documented proof of no backing allocation — `Vec`'s capacity field
    /// IS the size of its current allocation (0 means the dangling
    /// zero-capacity sentinel, never a live buffer), not a number reported
    /// independently of it. Two independently constructed placeholders
    /// sharing the exact same data pointer is the same fact restated
    /// empirically: a real allocator call would not hand back the same
    /// address twice for two simultaneously-live allocations.
    #[test]
    fn vacant_nursery_allocates_nothing() {
        let a = Nursery::vacant();
        let b = Nursery::vacant();
        assert_eq!(a.objects_capacity(), 0);
        assert_eq!(a.forwarding_capacity(), 0);
        assert_eq!(b.objects_capacity(), 0);
        assert_eq!(b.forwarding_capacity(), 0);
        assert_eq!(
            a.objects_data_ptr(),
            b.objects_data_ptr(),
            "two vacant nurseries have different data pointers — vacant() is allocating"
        );
    }
}
