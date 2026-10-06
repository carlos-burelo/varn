use super::cells::SlotState;
use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::closure::VmClosure;
use crate::value::VmValue;
use std::collections::hash_map::Entry;
use std::rc::Rc;
use std::sync::Arc;
use varn_types::value::{FrozenModuleObj, ModuleObj, RangeData, RuntimeSymbol};

impl HeapInner {
    pub(crate) fn alloc_vm_buffer(&mut self, buf: varn_types::VmBuffer) -> VmValue {
        let idx = self.alloc(HeapObj::Buffer(buf));
        VmValue::from_heap(idx)
    }

    pub(crate) fn alloc_symbol(&mut self, s: RuntimeSymbol) -> VmValue {
        let idx = match self.symbol_interner.entry(s.clone()) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => *e.insert(self.cells.alloc(HeapObj::Symbol(s), SlotState::Old)),
        };
        VmValue::from_heap(idx)
    }

    pub(crate) fn make_int(&mut self, n: i64) -> VmValue {
        VmValue::from_int(n)
    }

    pub(crate) fn alloc_spread(&mut self, inner: VmValue) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::Spread(inner)))
    }

    pub(crate) fn alloc_range_data(&mut self, r: varn_types::value::RangeData) -> VmValue {
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
        func: varn_types::NativeFn,
        name: &'static str,
    ) -> VmValue {
        let bound = varn_types::value::BoundMethod {
            receiver,
            target: varn_types::value::BoundMethodTarget::Native { func, name },
        };
        VmValue::from_heap(self.alloc(HeapObj::BoundMethod(Box::new(bound))))
    }

    pub(crate) fn alloc_bound_vm(
        &mut self,
        receiver: VmValue,
        closure: VmValue,
        owner_class: Option<Rc<varn_types::ClassObj>>,
    ) -> VmValue {
        let bound = varn_types::value::BoundMethod {
            receiver,
            target: varn_types::value::BoundMethodTarget::Vm {
                closure,
                owner_class,
            },
        };
        VmValue::from_heap(self.alloc(HeapObj::BoundMethod(Box::new(bound))))
    }

    pub(crate) fn alloc_enum_variant_vm(
        &mut self,
        data: varn_types::value::EnumVariantData,
    ) -> VmValue {
        VmValue::from_heap(self.alloc(HeapObj::EnumVariant(Box::new(data))))
    }

    pub(crate) fn alloc_set_vm(&mut self, set: varn_types::value::ValueSet) -> VmValue {
        let sref = varn_types::value::SetRef::new(set);
        VmValue::from_heap(self.alloc(HeapObj::Set(sref)))
    }

    pub(crate) fn alloc_class_vm(&mut self, class: Rc<varn_types::ClassObj>) -> VmValue {
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
