use super::cells::SlotState;
use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::value::VmValue;

impl HeapInner {
    pub(crate) fn alloc_char(&mut self, c: char) -> VmValue {
        if let Some(&idx) = self.char_interner.get(&c) {
            return VmValue::from_heap(idx);
        }
        let idx = self.cells.alloc(HeapObj::Char(c), SlotState::Old);
        self.note_old(idx);
        self.char_interner.insert(c, idx);
        VmValue::from_heap(idx)
    }

    pub(crate) fn alloc_bigint(&mut self, n: num_bigint::BigInt) -> VmValue {
        if let Some(&idx) = self.bigint_interner.get(&n) {
            return VmValue::from_heap(idx);
        }
        let idx = self
            .cells
            .alloc(HeapObj::BigInt(Box::new(n.clone())), SlotState::Old);
        self.note_old(idx);
        self.bigint_interner.insert(n, idx);
        VmValue::from_heap(idx)
    }

    pub(crate) fn intern_decimal(&mut self, d: bigdecimal::BigDecimal) -> VmValue {
        if let Some(&idx) = self.decimal_interner.get(&d) {
            return VmValue::from_heap(idx);
        }
        let idx = self
            .cells
            .alloc(HeapObj::Decimal(Box::new(d.clone())), SlotState::Old);
        self.note_old(idx);
        self.decimal_interner.insert(d, idx);
        VmValue::from_heap(idx)
    }
}
