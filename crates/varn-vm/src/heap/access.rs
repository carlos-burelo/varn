//! Reading a heap slot back out.
//!
//! Indices are PACKED: the high bit distinguishes a nursery index from an
//! old-generation one, so every accessor has to unpack before it indexes.
//! `get_raw*` take an already-unpacked old index and skip that step.

use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::nursery::{is_nursery_idx, old_idx_raw};
use crate::value::VmValue;
use std::rc::Rc;

impl HeapInner {
    #[inline(always)]
    pub(crate) fn get_by_idx(&self, idx: u32) -> Option<&HeapObj> {
        if is_nursery_idx(idx) {
            self.nursery.get(idx)
        } else {
            self.objects.get(old_idx_raw(idx) as usize)?.as_ref()
        }
    }

    #[inline(always)]
    pub(crate) fn get_by_idx_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        if is_nursery_idx(idx) {
            self.nursery.get_mut(idx)
        } else {
            self.objects.get_mut(old_idx_raw(idx) as usize)?.as_mut()
        }
    }

    #[inline(always)]
    pub(crate) fn get(&self, idx: u32) -> Option<&HeapObj> {
        self.get_by_idx(idx)
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, idx: u32) -> Option<&mut HeapObj> {
        self.get_by_idx_mut(idx)
    }

    #[inline(always)]
    pub(crate) fn get_raw(&self, raw_old_idx: u32) -> Option<&HeapObj> {
        self.objects.get(raw_old_idx as usize)?.as_ref()
    }

    #[inline(always)]
    pub(crate) fn get_raw_mut(&mut self, raw_old_idx: u32) -> Option<&mut HeapObj> {
        self.objects.get_mut(raw_old_idx as usize)?.as_mut()
    }

    pub(crate) fn class_idx(&self, class: &Rc<varn_types::ClassObj>) -> Option<u32> {
        self.identity_index
            .get(&(Rc::as_ptr(class) as usize))
            .copied()
    }

    pub(crate) fn closure_of(&self, v: VmValue) -> Option<&Rc<crate::closure::VmClosure>> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::VmClosure(c) => Some(c),
            _ => None,
        }
    }

    pub(crate) fn native_of(&self, v: VmValue) -> Option<(varn_types::NativeFn, &'static str)> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::NativeFn(f, name) => Some((*f, *name)),
            _ => None,
        }
    }

    pub(crate) fn symbol_of(&self, v: VmValue) -> Option<varn_types::value::RuntimeSymbol> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::Symbol(s) => Some(s.clone()),
            _ => None,
        }
    }

    pub(crate) fn generator_of(&self, v: VmValue) -> Option<varn_types::GeneratorObj> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::Generator(g) => Some(g.clone()),
            _ => None,
        }
    }

    pub(crate) fn char_of(&self, v: VmValue) -> Option<char> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::Char(c) => Some(*c),
            _ => None,
        }
    }

    pub(crate) fn bigint_of(&self, v: VmValue) -> Option<num_bigint::BigInt> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::BigInt(b) => Some((**b).clone()),
            _ => None,
        }
    }

    pub(crate) fn decimal_of(&self, v: VmValue) -> Option<bigdecimal::BigDecimal> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::Decimal(d) => Some((**d).clone()),
            _ => None,
        }
    }

    pub(crate) fn range_of(&self, v: VmValue) -> Option<varn_types::value::RangeData> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::Range(r) => Some(r.clone()),
            _ => None,
        }
    }

    pub(crate) fn map_of(&self, v: VmValue) -> Option<varn_types::value::MapRef> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::Map(m) => Some(m.clone()),
            _ => None,
        }
    }

    pub(crate) fn set_of(&self, v: VmValue) -> Option<varn_types::value::SetRef> {
        match self.get(self.get_heap_idx(v)?)? {
            HeapObj::Set(s) => Some(s.clone()),
            _ => None,
        }
    }

    pub(crate) fn is_static_receiver(&self, v: VmValue) -> bool {
        v.is_null()
            || self
                .get_heap_idx(v)
                .and_then(|idx| self.get(idx))
                .is_some_and(|o| matches!(o, HeapObj::Class(_) | HeapObj::Module(_)))
    }

    pub(crate) fn get_heap_idx(&self, val: VmValue) -> Option<u32> {
        if val.is_heap() {
            Some(val.as_heap_idx())
        } else {
            None
        }
    }
}
