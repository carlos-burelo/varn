use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::closure::VmClosure;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::Arc;
use vn_types::value::{FrozenModuleObj, ModuleObj, RangeData};

impl HeapInner {
    pub(crate) fn alloc_vm_buffer(&mut self, buf: vn_types::VmBuffer) -> VmValue {
        let idx = self.alloc(HeapObj::Buffer(buf));
        VmValue::from_heap(idx)
    }

    pub(crate) fn make_int(&mut self, n: i64) -> VmValue {
        VmValue::from_int(n)
    }

    pub(crate) fn alloc_spread(&mut self, inner: VmValue) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::Spread(inner)))
    }

    pub(crate) fn alloc_range_data(&mut self, r: vn_types::value::RangeData) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::Range(r)))
    }

    pub(crate) fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::Range(RangeData::int(start, end, inclusive))))
    }

    pub(crate) fn alloc_decimal(&mut self, d: bigdecimal::BigDecimal) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::Decimal(Box::new(d))))
    }

    pub(crate) fn alloc_bound_native(
        &mut self,
        receiver: VmValue,
        func: vn_types::NativeFn,
        name: &'static str,
    ) -> VmValue {
        let bound = vn_types::value::BoundMethod {
            receiver,
            target: vn_types::value::BoundMethodTarget::Native { func, name },
        };
        VmValue::from_heap(self.alloc(HeapObj::BoundMethod(Box::new(bound))))
    }

    pub(crate) fn alloc_bound_vm(
        &mut self,
        receiver: VmValue,
        closure: VmValue,
        owner_class: Option<Rc<vn_types::ClassObj>>,
    ) -> VmValue {
        let bound = vn_types::value::BoundMethod {
            receiver,
            target: vn_types::value::BoundMethodTarget::Vm {
                closure,
                owner_class,
            },
        };
        VmValue::from_heap(self.alloc(HeapObj::BoundMethod(Box::new(bound))))
    }

    pub(crate) fn alloc_enum_variant_vm(
        &mut self,
        data: vn_types::value::EnumVariantData,
    ) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::EnumVariant(Box::new(data))))
    }

    pub(crate) fn alloc_set_vm(&mut self, set: vn_types::value::ValueSet) -> VmValue {
        if let Some(h) = &self.hotspot {
            h.borrow_mut()
                .record_alloc(vn_core::RuntimeKind::Set.name());
        }
        let r = self.cells.alloc_set(set, super::cells::SlotState::Young);
        self.young.born.push(r);
        VmValue::from_heap(r)
    }

    pub(crate) fn alloc_class_vm(&mut self, class: Rc<vn_types::ClassObj>) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::Class(class)))
    }

    pub(crate) fn alloc_vm_closure(&mut self, c: Rc<VmClosure>) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::VmClosure(c)))
    }

    pub fn alloc_module(&mut self, m: Rc<ModuleObj>) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::Module(m)))
    }

    pub(crate) fn alloc_frozen_module(&mut self, m: Arc<FrozenModuleObj>) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::FrozenModule(m)))
    }
}
