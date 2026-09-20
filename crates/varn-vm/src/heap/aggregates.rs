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

    /// Construye un `ArrayRepr` tipado directamente desde el `TypeTag`
    /// angosto declarado (Task 3 del plan narrow-array-repr), en vez de
    /// inferir la representación por los valores como `alloc_array_vm`
    /// hace. `tag` nunca es `TypeTag::Null` (el llamador solo entra aquí
    /// cuando el byte de la instrucción es distinto de 0).
    pub(crate) fn alloc_array_vm_narrow(
        &mut self,
        items: Vec<VmValue>,
        tag: varn_core::TypeTag,
    ) -> VmValue {
        use varn_core::TypeTag as T;
        let va = match tag {
            T::I8 => VmArray::new_i8(items.iter().map(|v| v.as_int() as i8).collect()),
            T::I16 => VmArray::new_i16(items.iter().map(|v| v.as_int() as i16).collect()),
            T::I32 => VmArray::new_i32(items.iter().map(|v| v.as_int() as i32).collect()),
            T::U8 => VmArray::new_u8(items.iter().map(|v| v.as_int() as u8).collect()),
            T::U16 => VmArray::new_u16(items.iter().map(|v| v.as_int() as u16).collect()),
            T::U32 => VmArray::new_u32(items.iter().map(|v| v.as_int() as u32).collect()),
            T::F32 => VmArray::new_f32(items.iter().map(|v| v.as_f64() as f32).collect()),
            _ => return self.alloc_array_vm(items),
        };
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

    pub(crate) fn alloc_record_with_shape_slice(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: &[VmValue],
    ) -> VmValue {
        let oref = ObjRef::with_shape_slice(Rc::clone(shape), values);
        VmValue::from_heap_idx(self.alloc(HeapObj::Record(oref)))
    }

    pub(crate) fn alloc_empty_map_vm(&mut self) -> VmValue {
        let mref = match &self.empty_map {
            Some(m) => m.clone(),
            None => {
                let m = varn_types::value::MapRef::new(varn_types::value::ValueMap::default());
                self.empty_map = Some(m.clone());
                m
            }
        };
        VmValue::from_heap_idx(self.alloc(HeapObj::Map(mref)))
    }

    pub(crate) fn alloc_map_vm(&mut self, map: varn_types::value::ValueMap) -> VmValue {
        let mref = varn_types::value::MapRef::new(map);
        VmValue::from_heap_idx(self.alloc(HeapObj::Map(mref)))
    }
}
