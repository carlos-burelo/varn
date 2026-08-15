use super::shape::{root_shape, Shape};
use super::{ClassObj, RuntimeString, Value};
use crate::vm_value::{VmValue, VmValueRef};
use std::cell::{Cell, UnsafeCell};
use std::mem::MaybeUninit;
use std::ptr;
use std::rc::Rc;

/// A property object, stored as a single allocation: the header and the
/// object's fields share one `Rc` block, with the fields as a DST tail sized
/// to the shape the object was built with (V8's in-object slots).
///
/// Fields added *after* construction — the object grew past its original
/// shape — cannot extend the tail, so they spill into `overflow`. The
/// allocation therefore never moves, which object identity depends on:
/// `Value::Object` hashes and compares by `Rc` address.
///
/// Slot indices are flat across both stores: `slot < inline_len` reads the
/// tail, anything above reads `overflow[slot - inline_len]`. The JIT's inline
/// fast paths only know about the tail, so an overflowed slot fails their
/// bounds check and falls back to the interpreter helper.
///
/// `#[repr(C)]` pins the field order the JIT probes (`JitObjectLayout`).
#[repr(C)]
pub struct ObjData<T: ?Sized = [Cell<VmValue>]> {
    shape: UnsafeCell<Rc<Shape>>,
    inline_len: u32,
    _pad: u32,
    overflow: UnsafeCell<Option<Box<Vec<VmValue>>>>,
    values: T,
}

/// Header words preceding the tail. Asserted against the real layout below.
const HEADER_WORDS: usize = 3;

const _: () = {
    assert!(size_of::<VmValue>() == 8 && align_of::<VmValue>() == 8);
    assert!(size_of::<Cell<VmValue>>() == 8 && align_of::<Cell<VmValue>>() == 8);
    // The tail must start exactly HEADER_WORDS in, or `alloc` below hands `Rc`
    // a block of the wrong size and `drop` deallocates with the wrong layout.
    assert!(std::mem::offset_of!(ObjData<[Cell<VmValue>; 0]>, values) == HEADER_WORDS * 8);
    assert!(size_of::<ObjData<[Cell<VmValue>; 0]>>() == HEADER_WORDS * 8);
    assert!(align_of::<ObjData<[Cell<VmValue>; 0]>>() == 8);
};

impl ObjData {
    /// Allocates an object with `n` inline field slots, all null.
    ///
    /// The only unsafe construction in the object model. `Rc` cannot be handed
    /// a runtime-sized DST directly, so we allocate a `u64` slice whose block
    /// is byte-identical to `RcBox<ObjData<[Cell<VmValue>; n]>>` — header
    /// (24 bytes) plus `n` 8-byte slots — and re-point the fat pointer at it.
    /// The static asserts above are what make "byte-identical" true; `Rc`'s
    /// own drop then deallocates with `Layout::for_value`, which recomputes
    /// exactly this size from the tail length.
    pub fn alloc(shape: Rc<Shape>, n: usize) -> Rc<ObjData> {
        let backing: Rc<[MaybeUninit<u64>]> = Rc::new_uninit_slice(HEADER_WORDS + n);
        let base = Rc::into_raw(backing) as *const MaybeUninit<u64> as *mut Cell<VmValue>;
        let data = ptr::slice_from_raw_parts_mut(base, n) as *mut ObjData;

        unsafe {
            ptr::write(ptr::addr_of_mut!((*data).shape), UnsafeCell::new(shape));
            ptr::write(ptr::addr_of_mut!((*data).inline_len), n as u32);
            ptr::write(ptr::addr_of_mut!((*data)._pad), 0);
            ptr::write(ptr::addr_of_mut!((*data).overflow), UnsafeCell::new(None));

            let vals = ptr::addr_of_mut!((*data).values) as *mut Cell<VmValue>;
            for i in 0..n {
                ptr::write(vals.add(i), Cell::new(VmValue::null()));
            }

            Rc::from_raw(data as *const ObjData)
        }
    }

    /// Empty object on the root shape. Every field it later receives overflows.
    pub fn new() -> Rc<ObjData> {
        Self::alloc(root_shape(), 0)
    }

    /// Instance of `class`: the tail is sized to the class's declared fields,
    /// so a constructor's writes all land inline. This is the path the object
    /// allocation benchmark exercises.
    pub fn new_instance(class: Rc<ClassObj>) -> Rc<ObjData> {
        let shape = class.root_shape.borrow().clone();
        let n = shape.property_names.len();
        Self::alloc(shape, n)
    }

    /// Object literal with a statically known shape: one allocation, fields
    /// copied straight into the tail.
    pub fn with_shape(shape: Rc<Shape>, values: Vec<VmValue>) -> Rc<ObjData> {
        Self::with_shape_slice(shape, &values)
    }

    /// As [`Self::with_shape`], for callers that already hold the values in a
    /// buffer they own. The `Vec` form copies into the object's inline storage
    /// and then drops the `Vec`, so building one just to pass it here is a
    /// whole allocation with no purpose — which is what `JSON.parse` was doing
    /// once per object.
    pub fn with_shape_slice(shape: Rc<Shape>, values: &[VmValue]) -> Rc<ObjData> {
        let obj = Self::alloc(shape, values.len());
        for (i, v) in values.iter().enumerate() {
            obj.values[i].set(*v);
        }
        obj
    }

    /// Builds from key/value pairs, deriving the shape first so the whole
    /// object still fits in one allocation. Later duplicate keys overwrite
    /// earlier ones, as in an object literal.
    pub fn from_pairs<I>(pairs: I) -> Rc<ObjData>
    where
        I: IntoIterator<Item = (RuntimeString, VmValue)>,
    {
        let mut shape = root_shape();
        let mut values: Vec<VmValue> = Vec::new();
        for (k, v) in pairs {
            match shape.property_names.get(&k) {
                Some(&slot) => values[slot] = v,
                None => {
                    shape = shape.transition(k);
                    values.push(v);
                }
            }
        }
        Self::with_shape(shape, values)
    }
}

impl ObjData<[Cell<VmValue>]> {
    #[inline(always)]
    pub fn shape(&self) -> &Rc<Shape> {
        // Single-threaded by VM; a `&Rc<Shape>` handed out here is only held
        // across reads, never across `set_shape`.
        unsafe { &*self.shape.get() }
    }

    #[inline]
    pub fn set_shape(&self, shape: Rc<Shape>) {
        unsafe { *self.shape.get() = shape }
    }

    /// Number of slots living in the tail. The JIT reads this as its bounds
    /// check, which is what keeps it away from overflowed slots.
    #[inline(always)]
    pub fn inline_len(&self) -> usize {
        self.inline_len as usize
    }

    #[inline(always)]
    fn overflow(&self) -> Option<&Vec<VmValue>> {
        unsafe { (*self.overflow.get()).as_deref() }
    }

    #[inline]
    fn overflow_mut(&self) -> &mut Vec<VmValue> {
        unsafe { (*self.overflow.get()).get_or_insert_with(Box::default) }
    }

    /// Total addressable slots, tail plus overflow. Slot indices are flat
    /// across the two, so this is the bound every slot lookup checks.
    #[inline]
    pub fn slot_count(&self) -> usize {
        self.inline_len() + self.overflow().map_or(0, |o| o.len())
    }

    #[inline(always)]
    pub fn field_at(&self, slot: usize) -> Option<VmValue> {
        match self.values.get(slot) {
            Some(c) => Some(c.get()),
            None => self
                .overflow()
                .and_then(|o| o.get(slot - self.inline_len()))
                .copied(),
        }
    }

    /// Writes an existing slot. Returns false if `slot` addresses nothing —
    /// callers treat that as "not my field" rather than growing the object.
    #[inline(always)]
    pub fn set_field_at(&self, slot: usize, value: VmValue) -> bool {
        if let Some(c) = self.values.get(slot) {
            c.set(value);
            return true;
        }
        let off = slot - self.inline_len();
        let overflow = self.overflow_mut();
        match overflow.get_mut(off) {
            Some(s) => {
                *s = value;
                true
            }
            None => false,
        }
    }

    /// Visits every slot with its flat index — the GC's scan and the nursery's
    /// promotion fixups both walk objects through this.
    #[inline]
    pub fn for_each_field(&self, mut f: impl FnMut(usize, VmValue)) {
        for (i, c) in self.values.iter().enumerate() {
            f(i, c.get());
        }
        if let Some(o) = self.overflow() {
            let base = self.inline_len();
            for (i, v) in o.iter().enumerate() {
                f(base + i, *v);
            }
        }
    }

    #[inline]
    pub fn get(&self, name: &str) -> Option<VmValue> {
        let slot = self.shape().property_names.get(name).copied()?;
        self.field_at(slot)
    }

    /// Sets `name`, growing the object if it is not already in the shape.
    /// Growth spills to the overflow store: the allocation must not move,
    /// because `Value::Object` identity is its address.
    pub fn insert(&self, name: RuntimeString, value: VmValue) {
        if let Some(&slot) = self.shape().property_names.get(&name) {
            if self.set_field_at(slot, value) {
                return;
            }
            // Shape says the slot exists but no store backs it yet: the field
            // was added to the shape by a sibling object. Fall through and
            // extend the overflow up to it.
            let overflow = self.overflow_mut();
            overflow.resize(slot - self.inline_len() + 1, VmValue::null());
            overflow[slot - self.inline_len()] = value;
            return;
        }

        let new_shape = self.shape().transition(Rc::clone(&name));
        let slot = new_shape.property_names[&name];
        self.set_shape(new_shape);

        let base = self.inline_len();
        if slot < base {
            self.values[slot].set(value);
            return;
        }
        let overflow = self.overflow_mut();
        if slot - base >= overflow.len() {
            overflow.resize(slot - base + 1, VmValue::null());
        }
        overflow[slot - base] = value;
    }

    /// `delete obj.x`: rebuilds the shape without `name` and repacks the
    /// remaining fields into the same allocation.
    pub fn remove(&self, name: &str) -> Option<VmValue> {
        let removed_slot = *self.shape().property_names.get(name)?;
        let removed = self.field_at(removed_slot)?;

        // Kept in original slot order so the repacked object preserves the
        // field order the language exposes through `keys()`.
        let mut ordered: Vec<(RuntimeString, usize)> = self
            .shape()
            .property_names
            .iter()
            .filter(|(k, _)| k.as_ref() != name)
            .map(|(k, &slot)| (Rc::clone(k), slot))
            .collect();
        ordered.sort_unstable_by_key(|(_, slot)| *slot);

        let remaining: Vec<(RuntimeString, VmValue)> = ordered
            .into_iter()
            .map(|(k, slot)| (k, self.field_at(slot).unwrap_or(VmValue::null())))
            .collect();

        let mut new_shape = root_shape();
        for (k, _) in &remaining {
            new_shape = new_shape.transition(Rc::clone(k));
        }
        self.set_shape(new_shape);

        // Repack. The tail cannot shrink, so trailing slots are nulled rather
        // than left holding values the GC would keep alive.
        let base = self.inline_len();
        let overflow = self.overflow_mut();
        overflow.clear();
        for (i, (_, v)) in remaining.iter().enumerate() {
            if i < base {
                self.values[i].set(*v);
            } else {
                overflow.push(*v);
            }
        }
        for i in remaining.len()..base {
            self.values[i].set(VmValue::null());
        }

        Some(removed)
    }

    #[inline]
    pub fn contains_key(&self, name: &str) -> bool {
        self.shape().property_names.contains_key(name)
    }

    /// Number of fields the object exposes — the shape is the authority, not
    /// the tail, which keeps its size after a `remove`.
    #[inline]
    pub fn len(&self) -> usize {
        self.shape().property_names.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn keys(&self) -> std::vec::IntoIter<RuntimeString> {
        let mut pairs: Vec<(RuntimeString, usize)> = self
            .shape()
            .property_names
            .iter()
            .map(|(k, &idx)| (Rc::clone(k), idx))
            .collect();
        pairs.sort_unstable_by_key(|(_, idx)| *idx);
        pairs
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// The fields in declaration order, as owned pairs.
    ///
    /// Field order comes from [`Shape::ordered_names`], which the shape
    /// computed once. This used to sort a `Vec` built from the property
    /// HashMap and then collect it into a second `Vec` — two allocations, N
    /// `Rc` clones and a sort for every object visited, which `JSON.stringify`
    /// paid a million times over a 50 000-record document.
    ///
    /// Readers on a hot path should prefer walking `shape().ordered_names()`
    /// with [`Self::field_at`] directly: that allocates nothing at all.
    pub fn iter(&self) -> std::vec::IntoIter<(RuntimeString, VmValue)> {
        let names = self.shape().ordered_names();
        let mut pairs = Vec::with_capacity(names.len());
        for (slot, name) in names.iter().enumerate() {
            pairs.push((
                Rc::clone(name),
                self.field_at(slot).unwrap_or(VmValue::null()),
            ));
        }
        pairs.into_iter()
    }

    pub fn is_instance(&self) -> bool {
        self.shape().class.is_some()
    }

    pub fn class(&self) -> Option<Rc<ClassObj>> {
        self.shape().class.clone()
    }

    pub fn class_name(&self) -> String {
        match &self.shape().class {
            Some(c) => c.name.clone(),
            None => varn_core::TypeTag::Object.name().to_owned(),
        }
    }

    pub fn set_class(&self, class: Rc<ClassObj>) {
        let shape = Shape::create(Some(class), self.shape().property_names.clone());
        self.set_shape(shape);
    }

    pub fn get_field(&self, key: &str) -> Option<Value> {
        self.get(key).map(nv_to_value)
    }

    pub fn set_field(&self, key: RuntimeString, value: Value) {
        self.insert(key, value_to_nv(&value));
    }

    #[inline]
    pub fn get_field_nv(&self, key: &str) -> Option<VmValue> {
        self.get(key)
    }

    #[inline]
    pub fn set_field_nv(&self, key: RuntimeString, value: VmValue) {
        self.insert(key, value);
    }
}

impl std::fmt::Debug for ObjData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjData")
            .field("shape", self.shape())
            .field("inline_len", &self.inline_len())
            .field("slots", &self.slot_count())
            .finish()
    }
}

impl PartialEq for ObjData {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        for (k, v) in self.iter() {
            match other.get(&k) {
                Some(ov) if ov == v => {}
                _ => return false,
            }
        }
        true
    }
}
impl Eq for ObjData {}

#[inline]
pub fn nv_to_value(nv: VmValue) -> Value {
    if nv.is_null() {
        return Value::Null;
    }
    if nv.is_bool() {
        return Value::Bool(nv.as_bool());
    }
    if nv.is_int() {
        return Value::Int(nv.as_int());
    }
    if nv.is_f64() {
        return Value::Float(nv.as_f64());
    }
    if nv.is_sso() {
        let mut buf = [0u8; 5];
        return Value::Str(Rc::from(nv.sso_as_str(&mut buf)));
    }

    Value::VmValue(Box::new(VmValueRef(nv)))
}

#[inline]
pub fn value_to_nv(v: &Value) -> VmValue {
    match v {
        Value::Null => VmValue::null(),
        Value::Bool(b) => VmValue::from_bool(*b),
        Value::Int(i) => VmValue::from_int(*i),
        Value::Float(f) => VmValue::from_f64(*f),
        Value::Str(s) => {
            if let Some(nv) = VmValue::try_from_sso(s) {
                nv
            } else {
                debug_assert!(false, "set_field: long string '{}' must be pre-interned; use set_field_nv after heap.intern_str()", s);
                VmValue::null()
            }
        }
        Value::VmValue(payload) => {
            if let Some(vr) = payload.as_any().downcast_ref::<VmValueRef>() {
                vr.0
            } else {
                debug_assert!(
                    false,
                    "set_field: Value::VmValue with non-VmValueRef payload; use set_field_nv"
                );
                VmValue::null()
            }
        }
        other => {
            debug_assert!(
                false,
                "set_field: {:?} must be pre-interned; use set_field_nv after heap.intern()",
                other
            );
            VmValue::null()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_zero_sized_tail() {
        let o = ObjData::new();
        assert_eq!(o.inline_len(), 0);
        assert_eq!(o.slot_count(), 0);
        assert_eq!(o.len(), 0);
        assert!(o.is_empty());
    }

    #[test]
    fn empty_object_grows_entirely_into_overflow() {
        // `alloc_object()` (host builtins, isolate arg materialization) starts
        // with a zero-length tail, so every field it receives is an overflow
        // slot. Reading those back must work.
        let o = ObjData::new();
        assert_eq!(o.inline_len(), 0);

        o.insert(Rc::from("x"), VmValue::from_int(99));
        assert_eq!(o.slot_count(), 1);
        assert_eq!(o.field_at(0).unwrap().as_int(), 99);
        assert_eq!(o.get("x").unwrap().as_int(), 99);

        o.insert(Rc::from("y"), VmValue::from_int(7));
        assert_eq!(o.get("y").unwrap().as_int(), 7);

        let mut seen = Vec::new();
        o.for_each_field(|i, v| seen.push((i, v.as_int())));
        assert_eq!(seen, vec![(0, 99), (1, 7)]);
    }

    #[test]
    fn inline_fields_roundtrip() {
        let o = ObjData::from_pairs([
            (Rc::from("a"), VmValue::from_int(1)),
            (Rc::from("b"), VmValue::from_int(2)),
            (Rc::from("c"), VmValue::from_int(3)),
        ]);
        assert_eq!(o.inline_len(), 3);
        assert_eq!(o.slot_count(), 3);
        assert_eq!(o.get("a").unwrap().as_int(), 1);
        assert_eq!(o.get("b").unwrap().as_int(), 2);
        assert_eq!(o.get("c").unwrap().as_int(), 3);
        assert!(o.get("zz").is_none());

        o.insert(Rc::from("b"), VmValue::from_int(20));
        assert_eq!(o.get("b").unwrap().as_int(), 20);
        // Overwriting an existing field must not grow anything.
        assert_eq!(o.slot_count(), 3);
    }

    #[test]
    fn growth_spills_to_overflow_without_moving() {
        let o = ObjData::from_pairs([(Rc::from("a"), VmValue::from_int(1))]);
        let addr = Rc::as_ptr(&o) as *const u8;

        o.insert(Rc::from("b"), VmValue::from_int(2));
        o.insert(Rc::from("c"), VmValue::from_int(3));

        assert_eq!(Rc::as_ptr(&o) as *const u8, addr, "identity must be stable");
        assert_eq!(o.inline_len(), 1);
        assert_eq!(o.slot_count(), 3);
        assert_eq!(o.len(), 3);
        assert_eq!(o.get("a").unwrap().as_int(), 1);
        assert_eq!(o.get("b").unwrap().as_int(), 2);
        assert_eq!(o.get("c").unwrap().as_int(), 3);
    }

    #[test]
    fn for_each_field_visits_inline_and_overflow() {
        let o = ObjData::from_pairs([(Rc::from("a"), VmValue::from_int(1))]);
        o.insert(Rc::from("b"), VmValue::from_int(2));

        let mut seen = Vec::new();
        o.for_each_field(|i, v| seen.push((i, v.as_int())));
        assert_eq!(seen, vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn remove_repacks_and_nulls_the_tail() {
        let o = ObjData::from_pairs([
            (Rc::from("a"), VmValue::from_int(1)),
            (Rc::from("b"), VmValue::from_int(2)),
            (Rc::from("c"), VmValue::from_int(3)),
        ]);

        assert_eq!(o.remove("b").unwrap().as_int(), 2);
        assert_eq!(o.len(), 2);
        assert!(o.get("b").is_none());
        assert_eq!(o.get("a").unwrap().as_int(), 1);
        assert_eq!(o.get("c").unwrap().as_int(), 3);
        // The vacated tail slot must not keep the old value reachable.
        assert!(o.field_at(2).unwrap().is_null());
        assert!(o.remove("nope").is_none());
    }

    #[test]
    fn keys_and_iter_follow_slot_order() {
        let o = ObjData::from_pairs([
            (Rc::from("x"), VmValue::from_int(10)),
            (Rc::from("y"), VmValue::from_int(20)),
        ]);
        o.insert(Rc::from("z"), VmValue::from_int(30));

        let keys: Vec<String> = o.keys().map(|k| k.to_string()).collect();
        assert_eq!(keys, vec!["x", "y", "z"]);

        let vals: Vec<i64> = o.iter().map(|(_, v)| v.as_int()).collect();
        assert_eq!(vals, vec![10, 20, 30]);
    }

    #[test]
    fn drop_releases_shape_and_overflow() {
        let shape = root_shape();
        let before = Rc::strong_count(&shape);

        let o = ObjData::alloc(Rc::clone(&shape), 2);
        assert_eq!(
            Rc::strong_count(&shape),
            before + 1,
            "object holds its shape"
        );
        drop(o);
        assert_eq!(Rc::strong_count(&shape), before, "drop released the shape");

        // An object carrying an overflow store must drop cleanly too: the tail
        // and the boxed overflow are freed by one `Rc` deallocation.
        let grown = ObjData::from_pairs([(Rc::from("a"), VmValue::from_int(1))]);
        grown.insert(Rc::from("b"), VmValue::from_int(2));
        let grown_shape = Rc::clone(grown.shape());
        let held = Rc::strong_count(&grown_shape);
        drop(grown);
        assert_eq!(Rc::strong_count(&grown_shape), held - 1);
    }
}
