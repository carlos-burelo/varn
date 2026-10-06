use super::cells::SlotState;
use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::value::VmValue;
use std::collections::hash_map::Entry;

impl HeapInner {
    pub(crate) fn alloc_char(&mut self, c: char) -> VmValue {
        let idx = match self.char_interner.entry(c) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => *e.insert(self.cells.alloc(HeapObj::Char(c), SlotState::Old)),
        };
        VmValue::from_heap(idx)
    }

    pub(crate) fn alloc_bigint(&mut self, n: num_bigint::BigInt) -> VmValue {
        let idx = match self.bigint_interner.entry(n.clone()) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => *e.insert(
                self.cells
                    .alloc(HeapObj::BigInt(Box::new(n)), SlotState::Old),
            ),
        };
        VmValue::from_heap(idx)
    }

    pub(crate) fn intern_decimal(&mut self, d: bigdecimal::BigDecimal) -> VmValue {
        let idx = match self.decimal_interner.entry(d.clone()) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => *e.insert(
                self.cells
                    .alloc(HeapObj::Decimal(Box::new(d)), SlotState::Old),
            ),
        };
        VmValue::from_heap(idx)
    }
}
