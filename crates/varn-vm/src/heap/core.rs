//! The allocation core: how an object gets a heap slot.
//!
//! Every `alloc_*` in the sibling modules funnels through `alloc`, which puts
//! the object in the young generation unless its kind is born old. Interned
//! values take a slot directly through `SlotTable::alloc` as old.

use super::obj::HeapObj;
use super::slots::SlotState;
use super::structs::HeapInner;
use crate::value::VmValue;
use varn_types::NativeFn;

impl HeapInner {
    #[inline]
    pub(crate) fn alloc_native_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        VmValue::from_heap_idx(self.alloc(HeapObj::NativeFn(f, name)))
    }

    pub(crate) fn alloc(&mut self, obj: HeapObj) -> u32 {
        if let Some(h) = &self.hotspot {
            h.borrow_mut().record_alloc(obj.tag().name());
        }
        if Self::born_old(&obj) {
            return self.alloc_old(obj);
        }
        let idx = self.slots.alloc(obj, SlotState::Young);
        self.young.born.push(idx);
        self.young.alloc_count += 1;
        idx
    }

    fn alloc_old(&mut self, obj: HeapObj) -> u32 {
        let track = Self::needs_minor_scan(&obj);
        let identity = Self::identity_key(&obj);
        let idx = self.slots.alloc(obj, SlotState::Old);
        if track {
            self.scan_roots.push(idx);
        }
        if let Some(key) = identity {
            self.identity_index.insert(key, idx);
        }
        idx
    }
}
