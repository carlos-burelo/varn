use super::structs::HeapInner;
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::vm_value::ArrayRepr;

impl HeapInner {
    pub(crate) fn alloc_array_vm(&mut self, items: Vec<VmValue>) -> VmValue {
        self.alloc_array_repr(false, ArrayRepr::from_items(items))
    }

    pub(crate) fn alloc_array_slice_vm(&mut self, items: &[VmValue]) -> VmValue {
        self.alloc_array_repr(false, ArrayRepr::from_slice(items))
    }

    pub(crate) fn alloc_tuple_vm(&mut self, items: Vec<VmValue>) -> VmValue {
        self.alloc_array_repr(true, ArrayRepr::from_items(items))
    }

    pub(crate) fn alloc_object(&mut self) -> VmValue {
        self.alloc_object_cell(false, varn_types::root_shape(), 0, &[])
    }

    pub(crate) fn alloc_object_with_shape(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        self.alloc_object_with_shape_slice(shape, &values)
    }

    pub(crate) fn alloc_object_with_shape_slice(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: &[VmValue],
    ) -> VmValue {
        self.alloc_object_cell(false, Rc::clone(shape), values.len(), values)
    }

    pub(crate) fn alloc_record_with_shape_slice(
        &mut self,
        shape: &Rc<varn_types::Shape>,
        values: &[VmValue],
    ) -> VmValue {
        self.alloc_object_cell(true, Rc::clone(shape), values.len(), values)
    }

    pub(crate) fn alloc_empty_map_vm(&mut self) -> VmValue {
        self.alloc_map_vm(varn_types::value::ValueMap::default())
    }

    pub(crate) fn alloc_map_vm(&mut self, map: varn_types::value::ValueMap) -> VmValue {
        if let Some(h) = &self.hotspot {
            h.borrow_mut()
                .record_alloc(varn_core::RuntimeKind::Map.name());
        }
        let r = self.cells.alloc_map(map, super::cells::SlotState::Young);
        self.young.born.push(r);
        VmValue::from_heap(r)
    }
}
