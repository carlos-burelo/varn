//! Allocation for scalar and opaque heap values: symbols, ranges, decimals,
//! closures, modules and buffers.

use std::collections::hash_map::Entry;
use std::rc::Rc;
use std::sync::Arc;
use crate::closure::VmClosure;
use crate::nursery::pack_old_idx;
use crate::value::VmValue;
use varn_types::value::{
        FrozenModuleObj, ModuleObj, RangeData, RuntimeSymbol,
    };
use super::obj::HeapObj;
use super::core::alloc_into;
use super::structs::HeapInner;

impl HeapInner {
    pub(crate) fn alloc_vm_buffer(&mut self, buf: varn_types::VmBuffer) -> VmValue {
        let idx = self.alloc(HeapObj::Buffer(buf));
        VmValue::from_heap_idx(idx)
    }

    pub(crate) fn alloc_symbol(&mut self, s: RuntimeSymbol) -> VmValue {
        let packed = match self.symbol_interner.entry(s.clone()) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let packed = pack_old_idx(alloc_into(
                    &mut self.objects,
                    &mut self.free,
                    &mut self.alloc_count,
                    &mut self.gc_alloc_since_collect,
                    HeapObj::Symbol(s),
                ));
                *e.insert(packed)
            }
        };
        VmValue::from_heap_idx(packed)
    }

    pub(crate) fn make_int(&mut self, n: i64) -> VmValue {
        VmValue::from_int(n)
    }

    pub(crate) fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        let r = RangeData {
            start,
            end,
            inclusive,
            step: 1,
        };
        VmValue::from_heap_idx(self.alloc(HeapObj::Range(r)))
    }

    pub(crate) fn alloc_decimal(&mut self, d: rust_decimal::Decimal) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::Decimal(Box::new(d))))
    }

    pub(crate) fn alloc_vm_closure(&mut self, c: Rc<VmClosure>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::VmClosure(c)))
    }

    pub fn alloc_module(&mut self, m: Rc<ModuleObj>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::Module(m)))
    }

    pub(crate) fn alloc_frozen_module(&mut self, m: Arc<FrozenModuleObj>) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::FrozenModule(m)))
    }

}
