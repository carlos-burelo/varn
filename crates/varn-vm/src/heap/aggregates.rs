//! Allocation for arrays, tuples, objects and records — the values whose
//! representation is chosen at runtime from their contents.

use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::{value::ObjRef, Value, VmArray};

impl HeapInner {
    pub(crate) fn alloc_array_vm(&mut self, items: Vec<VmValue>) -> VmValue {
        let va = VmArray::from_items(items);
        VmValue::from_heap_idx(self.alloc(HeapObj::Array(va)))
    }

    pub(crate) fn alloc_tuple_vm(&mut self, items: Vec<VmValue>) -> VmValue {
        let va = VmArray::from_items(items);
        VmValue::from_heap_idx(self.alloc(HeapObj::Tuple(va)))
    }

    pub(crate) fn alloc_array(&mut self, items: Vec<Value>) -> VmValue {
        let vm_items: Vec<VmValue> = items.into_iter().map(|v| self.intern(v)).collect();
        self.alloc_array_vm(vm_items)
    }

    pub(crate) fn alloc_object(&mut self) -> VmValue {
        let oref = ObjRef::empty();
        VmValue::from_heap_idx(self.alloc(HeapObj::Object(oref)))
    }

    pub(crate) fn alloc_object_with_shape(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        self.alloc_object_with_shape_slice(shape, &values)
    }

    /// As [`Self::alloc_object_with_shape`], without requiring the caller to
    /// own a `Vec` it only builds to have it copied out and dropped.
    pub(crate) fn alloc_object_with_shape_slice(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: &[VmValue],
    ) -> VmValue {
        let oref = ObjRef::with_shape_slice(Rc::clone(shape), values);
        VmValue::from_heap_idx(self.alloc(HeapObj::Object(oref)))
    }

    pub(crate) fn alloc_record_with_shape(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        let oref = ObjRef::with_shape(Rc::clone(shape), values);
        VmValue::from_heap_idx(self.alloc(HeapObj::Record(oref)))
    }
}
