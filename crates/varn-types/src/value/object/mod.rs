use super::shape::{root_shape, Shape};
use super::{ClassObj, RuntimeString};
use crate::vm_value::VmValue;
use std::cell::{Cell, UnsafeCell};
use std::mem::MaybeUninit;
use std::ptr;
use std::rc::Rc;
use std::sync::Arc;

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
/// `#[repr(C)]` pins the field order the JIT derives (`JitObjectLayout`).
#[repr(C)]
pub struct ObjData<T: ?Sized = [Cell<VmValue>]> {
    shape: UnsafeCell<Rc<Shape>>,
    inline_len: u32,
    _pad: u32,
    overflow: UnsafeCell<Option<Box<Vec<VmValue>>>>,
    values: T,
}

/// Layout facts the JIT's inline property paths address directly. Derived
/// with `offset_of!` from this owned definition (exact by construction),
/// never re-measured by a scan. Field offsets before the tail are identical
/// for every tail length, so the zero-length instantiation answers for all.
pub const OBJ_SHAPE_OFF: usize = std::mem::offset_of!(ObjData<[Cell<VmValue>; 0]>, shape);
pub const OBJ_INLINE_LEN_OFF: usize = std::mem::offset_of!(ObjData<[Cell<VmValue>; 0]>, inline_len);
pub const OBJ_VALUES_OFF: usize = std::mem::offset_of!(ObjData<[Cell<VmValue>; 0]>, values);

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

mod access;
mod map_ops;
