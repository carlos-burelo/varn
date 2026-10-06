use super::cells::SlotState;
use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::HeapRef;

impl HeapInner {
    #[inline(always)]
    pub(crate) fn get(&self, r: HeapRef) -> Option<&HeapObj> {
        (self.cells.state(r) != SlotState::Free).then(|| self.cells.get(r))
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self, r: HeapRef) -> Option<&mut HeapObj> {
        if self.cells.state(r) == SlotState::Free {
            return None;
        }
        Some(self.cells.get_mut(r))
    }

    pub(crate) fn closure_of(&self, v: VmValue) -> Option<&Rc<crate::closure::VmClosure>> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::VmClosure(c) => Some(c),
            _ => None,
        }
    }

    pub(crate) fn native_of(&self, v: VmValue) -> Option<(varn_types::NativeFn, &'static str)> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::NativeFn(f, name) => Some((*f, *name)),
            _ => None,
        }
    }

    pub(crate) fn symbol_of(&self, v: VmValue) -> Option<varn_types::value::RuntimeSymbol> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::Symbol(s) => Some(s.clone()),
            _ => None,
        }
    }

    pub(crate) fn generator_of(&self, v: VmValue) -> Option<varn_types::GeneratorObj> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::Generator(g) => Some(g.clone()),
            _ => None,
        }
    }

    pub(crate) fn char_of(&self, v: VmValue) -> Option<char> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::Char(c) => Some(*c),
            _ => None,
        }
    }

    pub(crate) fn bigint_of(&self, v: VmValue) -> Option<num_bigint::BigInt> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::BigInt(b) => Some((**b).clone()),
            _ => None,
        }
    }

    pub(crate) fn decimal_of(&self, v: VmValue) -> Option<bigdecimal::BigDecimal> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::Decimal(d) => Some((**d).clone()),
            _ => None,
        }
    }

    pub(crate) fn range_of(&self, v: VmValue) -> Option<varn_types::value::RangeData> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::Range(r) => Some(r.clone()),
            _ => None,
        }
    }

    pub(crate) fn map_of(&self, v: VmValue) -> Option<varn_types::value::MapRef> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::Map(m) => Some(m.clone()),
            _ => None,
        }
    }

    pub(crate) fn set_of(&self, v: VmValue) -> Option<varn_types::value::SetRef> {
        match self.get(self.heap_ref(v)?)? {
            HeapObj::Set(s) => Some(s.clone()),
            _ => None,
        }
    }

    pub(crate) fn is_static_receiver(&self, v: VmValue) -> bool {
        v.is_null()
            || self
                .heap_ref(v)
                .and_then(|idx| self.get(idx))
                .is_some_and(|o| matches!(o, HeapObj::Class(_) | HeapObj::Module(_)))
    }

    pub(crate) fn heap_ref(&self, val: VmValue) -> Option<HeapRef> {
        if val.is_heap() {
            Some(val.as_heap())
        } else {
            None
        }
    }
}
