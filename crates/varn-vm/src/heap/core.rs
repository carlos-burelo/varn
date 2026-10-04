//! The allocation core: how an object gets a heap slot.
//!
//! Every `alloc_*` in the sibling modules funnels through `alloc`, which puts
//! the object in the young generation unless its kind is born old. Interned
//! values take a slot directly through `SlotTable::alloc` as old.

use super::cells::SlotState;
use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::value::VmValue;
use varn_types::value::{InstanceData, InstanceRef};
use varn_types::{ClassObj, HeapRef, NativeFn};

impl HeapInner {
    #[inline]
    pub(crate) fn alloc_native_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::NativeFn(f, name)))
    }

    pub(crate) fn alloc(&mut self, obj: HeapObj) -> HeapRef {
        if let Some(h) = &self.hotspot {
            h.borrow_mut().record_alloc(obj.tag().name());
        }
        if Self::born_old(&obj) {
            return self.alloc_old(obj);
        }
        let idx = self.cells.alloc(obj, SlotState::Young);
        self.young.born.push(idx);
        self.young.alloc_count += 1;
        idx
    }

    /// A new, zeroed instance of `class`, its payload in the same cell.
    pub(crate) fn alloc_instance(&mut self, class: &ClassObj) -> (HeapRef, InstanceRef) {
        if let Some(h) = &self.hotspot {
            h.borrow_mut().record_alloc("instance");
        }
        let size = InstanceData::payload_size_of(class);
        let (r, inst) = self.cells.alloc_instance(class.id, size, SlotState::Young);
        self.young.born.push(r);
        self.young.alloc_count += 1;
        (r, inst)
    }

    /// A new property object (`record` for a record) of `shape` with `n`
    /// inline slots, the first `values.len()` of them filled.
    pub(crate) fn alloc_object_cell(
        &mut self,
        record: bool,
        shape: std::rc::Rc<varn_types::Shape>,
        n: usize,
        values: &[VmValue],
    ) -> VmValue {
        if let Some(h) = &self.hotspot {
            h.borrow_mut()
                .record_alloc(if record { "record" } else { "object" });
        }
        let r = self
            .cells
            .alloc_object(record, shape, n, values, SlotState::Young);
        self.young.born.push(r);
        self.young.alloc_count += 1;
        VmValue::from_heap(r)
    }

    /// A new array (`tuple` for a tuple) holding `repr`.
    pub(crate) fn alloc_array_repr(
        &mut self,
        tuple: bool,
        repr: varn_types::vm_value::ArrayRepr,
    ) -> VmValue {
        if let Some(h) = &self.hotspot {
            h.borrow_mut()
                .record_alloc(if tuple { "tuple" } else { "array" });
        }
        let r = self.cells.alloc_array(tuple, repr, SlotState::Young);
        self.young.born.push(r);
        self.young.alloc_count += 1;
        VmValue::from_heap(r)
    }

    /// An object literal with these key/value pairs.
    pub(crate) fn alloc_object_pairs<I>(&mut self, pairs: I) -> VmValue
    where
        I: IntoIterator<Item = (varn_types::RuntimeString, VmValue)>,
    {
        let (shape, values) = varn_types::value::ObjData::pairs_layout(pairs);
        self.alloc_object_cell(false, shape, values.len(), &values)
    }

    fn alloc_old(&mut self, obj: HeapObj) -> HeapRef {
        let track = Self::needs_minor_scan(&obj);
        let identity = Self::identity_key(&obj);
        let idx = self.cells.alloc(obj, SlotState::Old);
        if track {
            self.scan_roots.push(idx);
        }
        if let Some(key) = identity {
            self.identity_index.insert(key, idx);
        }
        idx
    }
}
