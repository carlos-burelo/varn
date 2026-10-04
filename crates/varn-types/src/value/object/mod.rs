use super::shape::{root_shape, Shape};
use super::{ObjRef, RuntimeString};
use crate::vm_value::VmValue;
use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::rc::Rc;
use std::sync::Arc;

/// A property object, stored in one heap cell: the header and the object's
/// fields share the cell, with the fields as a DST tail sized
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
    /// Bytes an object with `n` inline field slots occupies, header included.
    pub const fn bytes_for(n: usize) -> usize {
        (HEADER_WORDS + n * WORDS_PER_VALUE) * size_of::<u64>()
    }

    /// Lays out an object at `at` with `n` inline slots: the first
    /// `values.len()` hold `values`, the rest `null`.
    ///
    /// # Safety
    /// `at` must be 8-aligned, point to [`Self::bytes_for`]`(n)` writable
    /// bytes, and outlive every use of the returned reference: the heap cell
    /// that holds the object owns that memory and runs [`Self::drop_at`].
    pub unsafe fn init_at(at: *mut u8, shape: Rc<Shape>, n: usize, values: &[VmValue]) -> ObjRef {
        debug_assert!(values.len() <= n);
        let data = ptr::slice_from_raw_parts_mut(at as *mut Cell<VmValue>, n) as *mut ObjData;
        ptr::write(ptr::addr_of_mut!((*data).shape), UnsafeCell::new(shape));
        ptr::write(ptr::addr_of_mut!((*data).inline_len), n as u32);
        ptr::write(ptr::addr_of_mut!((*data)._pad), 0);
        ptr::write(ptr::addr_of_mut!((*data).overflow), UnsafeCell::new(None));
        let vals = ptr::addr_of_mut!((*data).values) as *mut Cell<VmValue>;
        ptr::write_bytes(vals, 0, n);
        for (i, v) in values.iter().enumerate() {
            (*vals.add(i)).set(*v);
        }
        ObjRef(ptr::NonNull::new_unchecked(data))
    }

    /// Releases what an object owns outside its cell: its shape and its
    /// overflow store.
    ///
    /// # Safety
    /// `obj` must have come from [`Self::init_at`] and never be used again.
    pub unsafe fn drop_at(obj: ObjRef) {
        ptr::drop_in_place(obj.0.as_ptr());
    }

    /// The shape and values an object literal with these key/value pairs has.
    /// Later duplicate keys overwrite earlier ones, as in an object literal.
    pub fn pairs_layout<I>(pairs: I) -> (Rc<Shape>, Vec<VmValue>)
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
        (shape, values)
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
