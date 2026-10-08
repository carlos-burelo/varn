use super::shape::{root_shape, Shape};
use super::{ObjRef, RuntimeString};
use crate::vm_value::VmValue;
use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::rc::Rc;
use std::sync::Arc;

#[repr(C)]
pub struct ObjData<T: ?Sized = [Cell<VmValue>]> {
    shape: UnsafeCell<Rc<Shape>>,
    inline_len: u32,
    _pad: u32,
    overflow: UnsafeCell<Option<Box<Vec<VmValue>>>>,
    values: T,
}

pub const OBJ_SHAPE_OFF: usize = std::mem::offset_of!(ObjData<[Cell<VmValue>; 0]>, shape);
pub const OBJ_INLINE_LEN_OFF: usize = std::mem::offset_of!(ObjData<[Cell<VmValue>; 0]>, inline_len);
pub const OBJ_VALUES_OFF: usize = std::mem::offset_of!(ObjData<[Cell<VmValue>; 0]>, values);

const HEADER_WORDS: usize = 3;

const WORDS_PER_VALUE: usize = size_of::<Cell<VmValue>>() / size_of::<u64>();

const _: () = {
    assert!(size_of::<Cell<VmValue>>().is_multiple_of(size_of::<u64>()));
    assert!(align_of::<VmValue>() == 8);
    assert!(align_of::<Cell<VmValue>>() == 8);
    assert!(size_of::<Cell<VmValue>>() == size_of::<VmValue>());

    assert!(std::mem::offset_of!(ObjData<[Cell<VmValue>; 0]>, values) == HEADER_WORDS * 8);
    assert!(size_of::<ObjData<[Cell<VmValue>; 0]>>() == HEADER_WORDS * 8);
    assert!(align_of::<ObjData<[Cell<VmValue>; 0]>>() == 8);
};

impl ObjData {
    pub const fn bytes_for(n: usize) -> usize {
        (HEADER_WORDS + n * WORDS_PER_VALUE) * size_of::<u64>()
    }

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

    pub unsafe fn drop_at(obj: ObjRef) {
        ptr::drop_in_place(obj.0.as_ptr());
    }

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
                Some(_) | None => return false,
            }
        }
        true
    }
}
impl Eq for ObjData {}

mod access;
mod map_ops;
