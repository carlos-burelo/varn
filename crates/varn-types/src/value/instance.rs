use super::ClassObj;
use crate::vm_value::VmValue;
use std::cell::UnsafeCell;
use std::ptr;
use std::rc::Rc;
use varn_core::layout::{ClassLayout, FieldLayout, ScalarRepr, TypeLayout, COMPACT_REF_NULL};

#[repr(C, align(8))]
pub struct InstanceData<T: ?Sized = [UnsafeCell<u8>]> {
    pub class_id: u32,
    pub payload_size: u32,
    payload: T,
}

pub const INST_CLASS_ID_OFF: usize =
    std::mem::offset_of!(InstanceData<[UnsafeCell<u8>; 0]>, class_id);
pub const INST_PAYLOAD_OFF: usize =
    std::mem::offset_of!(InstanceData<[UnsafeCell<u8>; 0]>, payload);

impl InstanceData {
    pub fn payload_size_of(class: &ClassObj) -> u32 {
        class.layout().payload_size
    }

    pub const fn bytes_for(payload_size: u32) -> usize {
        INST_PAYLOAD_OFF + (payload_size as usize).div_ceil(8) * 8
    }

    #[inline]
    pub unsafe fn init_at(at: *mut u8, class_id: u32, payload_size: u32) -> InstanceRef {
        let payload_bytes = payload_size as usize;
        let data = ptr::slice_from_raw_parts_mut(at as *mut UnsafeCell<u8>, payload_bytes)
            as *mut InstanceData;
        ptr::write(ptr::addr_of_mut!((*data).class_id), class_id);
        ptr::write(ptr::addr_of_mut!((*data).payload_size), payload_size);
        let payload_ptr = ptr::addr_of_mut!((*data).payload) as *mut u8;
        ptr::write_bytes(payload_ptr, 0, payload_bytes);
        InstanceRef(ptr::NonNull::new_unchecked(data))
    }

    #[inline(always)]
    pub fn raw_payload_ptr(&self) -> *mut u8 {
        self.payload.as_ptr() as *mut u8
    }

    #[inline(always)]
    unsafe fn read_i64(&self, offset: usize) -> i64 {
        let ptr = self.raw_payload_ptr().add(offset) as *const i64;
        ptr.read()
    }

    #[inline(always)]
    unsafe fn read_f64(&self, offset: usize) -> f64 {
        let ptr = self.raw_payload_ptr().add(offset) as *const f64;
        ptr.read()
    }

    #[inline(always)]
    unsafe fn read_bool(&self, offset: usize) -> bool {
        let ptr = self.raw_payload_ptr().add(offset);
        *ptr != 0
    }

    #[inline(always)]
    unsafe fn read_u64(&self, offset: usize) -> u64 {
        let ptr = self.raw_payload_ptr().add(offset) as *const u64;
        ptr.read()
    }

    #[inline(always)]
    unsafe fn read_vm_value(&self, offset: usize) -> VmValue {
        let ptr = self.raw_payload_ptr().add(offset) as *const VmValue;
        ptr.read()
    }

    #[inline]
    fn layout(&self) -> Option<Rc<ClassLayout>> {
        ClassObj::find_by_id(self.class_id).map(|c| c.layout())
    }

    #[inline]
    pub fn slot_count(&self) -> usize {
        self.layout().map(|l| l.field_count()).unwrap_or(0)
    }

    #[inline]
    pub fn field_at(&self, slot: usize) -> Option<VmValue> {
        let layout = self.layout()?;
        let f = layout.get_field_by_index(slot)?;
        self.read_field(f)
    }

    #[inline]
    pub fn read_field_at(
        &self,
        offset: u32,
        tag: Option<varn_core::RuntimeKind>,
    ) -> Option<VmValue> {
        self.read_scalar(offset, &TypeLayout::of_field(tag))
    }

    #[inline]
    pub fn write_field_at(
        &self,
        offset: u32,
        tag: Option<varn_core::RuntimeKind>,
        val: VmValue,
    ) -> Result<(), &'static str> {
        self.write_scalar(offset, &TypeLayout::of_field(tag), val)
    }

    #[inline]
    pub fn set_field_at(&self, slot: usize, val: VmValue) -> bool {
        let Some(layout) = self.layout() else {
            return false;
        };
        let Some(f) = layout.get_field_by_index(slot) else {
            return false;
        };
        self.write_field(f, val).is_ok()
    }

    pub fn read_field(&self, f: &FieldLayout) -> Option<VmValue> {
        self.read_scalar(f.offset, &f.layout)
    }

    pub fn read_scalar(&self, offset: u32, layout: &TypeLayout) -> Option<VmValue> {
        let offset = offset as usize;
        if offset + layout.size as usize > self.payload_size as usize {
            return None;
        }
        unsafe {
            Some(match layout.repr {
                ScalarRepr::Bool => VmValue::from_bool(self.read_bool(offset)),
                ScalarRepr::I64 => VmValue::from_int(self.read_i64(offset)),
                ScalarRepr::F64 => VmValue::from_f64(self.read_f64(offset)),
                ScalarRepr::Ref => {
                    let raw = self.read_u64(offset);
                    if raw == COMPACT_REF_NULL {
                        VmValue::null()
                    } else {
                        VmValue::from_heap(crate::HeapRef::from_addr_unchecked(raw))
                    }
                }
                ScalarRepr::Boxed => self.read_vm_value(offset),
            })
        }
    }

    pub fn write_field(&self, f: &FieldLayout, val: VmValue) -> Result<(), &'static str> {
        self.write_scalar(f.offset, &f.layout, val)
    }

    pub fn write_scalar(
        &self,
        offset: u32,
        layout: &TypeLayout,
        val: VmValue,
    ) -> Result<(), &'static str> {
        let offset = offset as usize;
        if offset + layout.size as usize > self.payload_size as usize {
            return Err("field offset exceeds instance payload");
        }
        unsafe {
            match layout.repr {
                ScalarRepr::Bool => {
                    if !val.is_bool() {
                        return Err("cannot store non-bool in a bool field");
                    }
                    self.write_bool(offset, val.as_bool());
                }
                ScalarRepr::I64 => {
                    if !val.is_int() {
                        return Err("cannot store non-int in an int field");
                    }
                    self.write_i64(offset, val.as_int());
                }
                ScalarRepr::F64 => {
                    if val.is_f64() {
                        self.write_f64(offset, val.as_f64());
                    } else if val.is_int() {
                        self.write_f64(offset, val.as_int() as f64);
                    } else if val.is_null() {
                        self.write_f64(offset, f64::NAN);
                    } else {
                        return Err("cannot store non-numeric in a float field");
                    }
                }
                ScalarRepr::Ref => {
                    if val.is_null() {
                        self.write_u64(offset, COMPACT_REF_NULL);
                    } else if val.is_heap() {
                        self.write_u64(offset, val.as_heap().addr());
                    } else {
                        return Err("cannot store non-reference in a reference field");
                    }
                }
                ScalarRepr::Boxed => self.write_vm_value(offset, val),
            }
        }
        Ok(())
    }

    pub fn for_each_reference(&self, mut f: impl FnMut(VmValue)) {
        let Some(layout) = self.layout() else {
            return;
        };
        for slot in &layout.gc.slots {
            if let Some(v) = self.read_gc_slot(slot.offset as usize, slot.repr) {
                f(v);
            }
        }
    }

    fn read_gc_slot(&self, offset: usize, repr: ScalarRepr) -> Option<VmValue> {
        unsafe {
            match repr {
                ScalarRepr::Ref => {
                    let raw = self.read_u64(offset);
                    crate::HeapRef::from_addr(raw).map(VmValue::from_heap)
                }
                ScalarRepr::Boxed => Some(self.read_vm_value(offset)),
                ScalarRepr::Bool | ScalarRepr::I64 | ScalarRepr::F64 => {
                    unreachable!("GcLayout lists only Ref and Boxed slots")
                }
            }
        }
    }

    #[inline(always)]
    unsafe fn write_i64(&self, offset: usize, val: i64) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut i64;
        ptr.write(val);
    }

    #[inline(always)]
    unsafe fn write_f64(&self, offset: usize, val: f64) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut f64;
        ptr.write(val);
    }

    #[inline(always)]
    unsafe fn write_bool(&self, offset: usize, val: bool) {
        let ptr = self.raw_payload_ptr().add(offset);
        *ptr = val as u8;
    }

    #[inline(always)]
    unsafe fn write_u64(&self, offset: usize, val: u64) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut u64;
        ptr.write(val);
    }

    #[inline(always)]
    unsafe fn write_vm_value(&self, offset: usize, val: VmValue) {
        let ptr = self.raw_payload_ptr().add(offset) as *mut VmValue;
        ptr.write(val);
    }
}

#[derive(Clone, Copy)]
pub struct InstanceRef(ptr::NonNull<InstanceData>);

impl InstanceRef {
    #[inline(always)]
    pub fn read(&self) -> &InstanceData {
        unsafe { self.0.as_ref() }
    }
}

impl std::ops::Deref for InstanceRef {
    type Target = InstanceData;
    #[inline(always)]
    fn deref(&self) -> &InstanceData {
        unsafe { self.0.as_ref() }
    }
}

impl std::fmt::Debug for InstanceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceData")
            .field("class_id", &self.class_id)
            .field("payload_size", &self.payload_size)
            .finish()
    }
}

impl std::fmt::Debug for InstanceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "InstanceRef(class_id={}, size={})",
            self.class_id, self.payload_size
        )
    }
}

impl PartialEq for InstanceRef {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        ptr::addr_eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

impl Eq for InstanceRef {}

impl std::hash::Hash for InstanceRef {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.0.as_ptr() as *const u8).hash(state);
    }
}
