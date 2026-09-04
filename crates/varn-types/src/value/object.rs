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

/// `u64` words one field slot occupies. Derived, not written down: the tail is
/// allocated as a `u64` slice, so this is what converts a field count into a
/// word count. It was 1 when a value was a NaN-boxed `u64`; it is 2 now that a
/// value is a tag word plus a payload word.
const WORDS_PER_VALUE: usize = size_of::<Cell<VmValue>>() / size_of::<u64>();

const _: () = {
    // The tail is carved out of a `u64` slice, so a value must be a whole
    // number of words and must not need stricter alignment than one.
    assert!(size_of::<Cell<VmValue>>().is_multiple_of(size_of::<u64>()));
    assert!(align_of::<VmValue>() == 8);
    assert!(align_of::<Cell<VmValue>>() == 8);
    assert!(size_of::<Cell<VmValue>>() == size_of::<VmValue>());
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
    /// (24 bytes) plus `n` slots of [`WORDS_PER_VALUE`] words each — and
    /// re-point the fat pointer at it. The static asserts above are what make
    /// "byte-identical" true; `Rc`'s own drop then deallocates with
    /// `Layout::for_value`, which recomputes exactly this size from the tail
    /// length.
    pub fn alloc(shape: Rc<Shape>, n: usize) -> Rc<ObjData> {
        let backing: Rc<[MaybeUninit<u64>]> =
            Rc::new_uninit_slice(HEADER_WORDS + n * WORDS_PER_VALUE);
        let base = Rc::into_raw(backing) as *const MaybeUninit<u64> as *mut Cell<VmValue>;
        let data = ptr::slice_from_raw_parts_mut(base, n) as *mut ObjData;

        unsafe {
            ptr::write(ptr::addr_of_mut!((*data).shape), UnsafeCell::new(shape));
            ptr::write(ptr::addr_of_mut!((*data).inline_len), n as u32);
            ptr::write(ptr::addr_of_mut!((*data)._pad), 0);
            ptr::write(ptr::addr_of_mut!((*data).overflow), UnsafeCell::new(None));

            let vals = ptr::addr_of_mut!((*data).values) as *mut Cell<VmValue>;
            if n > 0 {
                // VmValue::null() is bitwise identical to all-zeros (tag: 0, payload: 0).
                ptr::write_bytes(vals, 0, n);
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
    pub fn new_instance(class: &ClassObj) -> Rc<ObjData> {
        let (shape, n) = class.instance_shape();
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

    /// Direct reference to the object's inline fields buffer.
    #[inline(always)]
    pub fn inline_slice(&self) -> &[Cell<VmValue>] {
        &self.values
    }

    #[inline(always)]
    fn overflow(&self) -> Option<&Vec<VmValue>> {
        unsafe { (*self.overflow.get()).as_deref() }
    }

    /// `&mut` out of `&self` is the `UnsafeCell` contract this object layout is
    /// built on — see `ArrayRef::write` for the full argument. Allowed at the
    /// site so a new one elsewhere still has to justify itself.
    #[allow(clippy::mut_from_ref)]
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
        let shape = self.shape();
        let ordered = shape.ordered_names();
        if ordered.len() <= 4 {
            for (slot, prop_name) in ordered.iter().enumerate() {
                if prop_name.as_ref() == name {
                    return self.field_at(slot);
                }
            }
            return None;
        }
        let slot = shape.property_names.get(name).copied()?;
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
        let shape = self.shape();
        let ordered = shape.ordered_names();
        if ordered.len() <= 4 {
            return ordered.iter().any(|p| p.as_ref() == name);
        }
        shape.property_names.contains_key(name)
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

    #[inline]
    pub fn set_field_str(&self, key: &str, value: VmValue) {
        let shape = self.shape();
        let ordered = shape.ordered_names();
        let existing_slot = if ordered.len() <= 4 {
            ordered.iter().position(|p| p.as_ref() == key)
        } else {
            shape.property_names.get(key).copied()
        };
        if let Some(slot) = existing_slot {
            if self.set_field_at(slot, value) {
                return;
            }
            let overflow = self.overflow_mut();
            overflow.resize(slot - self.inline_len() + 1, VmValue::null());
            overflow[slot - self.inline_len()] = value;
            return;
        }

        let new_shape = self.shape().transition_str(key);
        let slot = new_shape.property_names[key];
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

// ── Native Static Class Instance (InstanceData) ─────────────────────────

/// Native static struct representation of a user class instance.
///
/// Unlike dynamic `ObjData`, an `InstanceData`:
/// - Has NO `Shape` pointer (classes have static layout).
/// - Has NO `overflow` store (no dynamic property additions).
/// - Has a compact 8-byte header (`class_id: u32`, `payload_size: u32`).
/// - Stores primitive fields packed at native offsets without 16-byte VmValue boxing.
#[repr(C)]
pub struct InstanceData<T: ?Sized = [UnsafeCell<u8>]> {
    pub class: Rc<crate::value::ClassObj>,
    pub payload_size: u32,
    payload: T,
}

impl InstanceData {
    /// Allocates an `InstanceData` on the heap with the specified layout.
    pub fn alloc(class: Rc<crate::value::ClassObj>) -> Rc<InstanceData> {
        let layout = class.get_or_compute_layout();
        let payload_bytes = layout.payload_size as usize;
        let payload_words = (payload_bytes + 7) / 8;
        let header_words = (std::mem::size_of::<InstanceData<[UnsafeCell<u8>; 0]>>() + 7) / 8;
        let total_words = header_words + payload_words;

        let backing: Rc<[MaybeUninit<u64>]> = Rc::new_uninit_slice(total_words);
        let base = Rc::into_raw(backing) as *const MaybeUninit<u64> as *mut UnsafeCell<u8>;
        let data = ptr::slice_from_raw_parts_mut(base, payload_bytes) as *mut InstanceData;

        unsafe {
            ptr::write(ptr::addr_of_mut!((*data).class), class);
            ptr::write(ptr::addr_of_mut!((*data).payload_size), layout.payload_size);

            let payload_ptr = ptr::addr_of_mut!((*data).payload) as *mut u8;
            if payload_bytes > 0 {
                ptr::write_bytes(payload_ptr, 0, payload_bytes);
            }

            Rc::from_raw(data as *const InstanceData)
        }
    }

    #[inline(always)]
    pub fn raw_payload_ptr(&self) -> *mut u8 {
        self.payload.as_ptr() as *mut u8
    }

    // ── Field Read Methods ──────────────────────────────────────────────

    #[inline(always)]
    pub unsafe fn read_i64(&self, offset: usize) -> i64 {
        let ptr = self.raw_payload_ptr().add(offset) as *const i64;
        ptr.read()
    }

    #[inline(always)]
    pub unsafe fn read_f64(&self, offset: usize) -> f64 {
        let ptr = self.raw_payload_ptr().add(offset) as *const f64;
        ptr.read()
    }

    #[inline(always)]
    pub unsafe fn read_bool(&self, offset: usize) -> bool {
        let ptr = self.raw_payload_ptr().add(offset);
        *ptr != 0
    }

    #[inline(always)]
    pub unsafe fn read_u8(&self, offset: usize) -> u8 {
        let ptr = self.raw_payload_ptr().add(offset);
        *ptr
    }

    #[inline(always)]
    pub unsafe fn read_u32(&self, offset: usize) -> u32 {
        let ptr = self.raw_payload_ptr().add(offset) as *const u32;
        ptr.read()
    }

    #[inline(always)]
    pub unsafe fn read_vm_value(&self, offset: usize) -> VmValue {
        let ptr = self.raw_payload_ptr().add(offset) as *const VmValue;
        ptr.read()
    }

    #[inline(always)]
    pub fn get(&self, key: &str) -> Option<VmValue> {
        let layout = self.class.get_or_compute_layout();
        let f = layout.get_field(key)?;
        let offset = f.offset as usize;
        if offset + 16 <= self.payload_size as usize {
            Some(unsafe { self.read_vm_value(offset) })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn set(&self, key: &str, val: VmValue) -> bool {
        let layout = self.class.get_or_compute_layout();
        let f = match layout.get_field(key) {
            Some(f) => f,
            None => return false,
        };
        let offset = f.offset as usize;
        if offset + 16 <= self.payload_size as usize {
            unsafe { self.write_vm_value(offset, val) };
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn field_at(&self, slot: usize) -> Option<VmValue> {
        let offset = slot * 16;
        if offset + 16 <= self.payload_size as usize {
            Some(unsafe { self.read_vm_value(offset) })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn set_field_at(&self, slot: usize, val: VmValue) -> bool {
        let offset = slot * 16;
        if offset + 16 <= self.payload_size as usize {
            unsafe { self.write_vm_value(offset, val) };
            true
        } else {
            false
        }
    }

    // ── Field Write Methods ─────────────────────────────────────────────

    #[inline(always)]
    pub unsafe fn write_i64(&self, offset: usize, val: i64) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut i64;
        ptr.write(val);
    }

    #[inline(always)]
    pub unsafe fn write_f64(&self, offset: usize, val: f64) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut f64;
        ptr.write(val);
    }

    #[inline(always)]
    pub unsafe fn write_bool(&self, offset: usize, val: bool) {
        let ptr = self.raw_payload_ptr().add(offset);
        *ptr = val as u8;
    }

    #[inline(always)]
    pub unsafe fn write_u8(&self, offset: usize, val: u8) {
        let ptr = self.raw_payload_ptr().add(offset);
        *ptr = val;
    }

    #[inline(always)]
    pub unsafe fn write_u32(&self, offset: usize, val: u32) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut u32;
        ptr.write(val);
    }

    #[inline(always)]
    pub unsafe fn write_vm_value(&self, offset: usize, val: VmValue) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut VmValue;
        ptr.write(val);
    }
}

impl<T: ?Sized> Drop for InstanceData<T> {
    fn drop(&mut self) {
        unsafe {
            ptr::drop_in_place(&mut self.class);
        }
    }
}

/// Reference-counted wrapper around [`InstanceData`].
#[derive(Clone)]
pub struct InstanceRef(pub Rc<InstanceData>);

impl InstanceRef {
    #[inline]
    pub fn alloc(class: Rc<crate::value::ClassObj>) -> Self {
        Self(InstanceData::alloc(class))
    }

    #[inline(always)]
    pub fn read(&self) -> &InstanceData {
        &self.0
    }
}

impl std::ops::Deref for InstanceRef {
    type Target = InstanceData;
    #[inline(always)]
    fn deref(&self) -> &InstanceData {
        &self.0
    }
}

impl std::fmt::Debug for InstanceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceData")
            .field("class", &self.class.name)
            .field("payload_size", &self.payload_size)
            .finish()
    }
}

impl std::fmt::Debug for InstanceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InstanceRef(class={}, size={})", self.0.class.name, self.0.payload_size)
    }
}

impl PartialEq for InstanceRef {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for InstanceRef {}

impl std::hash::Hash for InstanceRef {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}
