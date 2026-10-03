//! Content-interned scalars: a `char`, `bigint` or `decimal` with the same
//! value always lives in one heap slot, which is what lets map and set keys
//! compare by identity.

use super::core::alloc_into;
use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::nursery::pack_old_idx;
use crate::value::VmValue;
use std::collections::hash_map::Entry;

impl HeapInner {
    pub(crate) fn alloc_char(&mut self, c: char) -> VmValue {
        let packed = match self.char_interner.entry(c) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                HeapObj::Char(c),
            ))),
        };
        VmValue::from_heap_idx(packed)
    }

    pub(crate) fn alloc_bigint(&mut self, n: num_bigint::BigInt) -> VmValue {
        let packed = match self.bigint_interner.entry(n.clone()) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                HeapObj::BigInt(Box::new(n)),
            ))),
        };
        VmValue::from_heap_idx(packed)
    }

    pub(crate) fn intern_decimal(&mut self, d: bigdecimal::BigDecimal) -> VmValue {
        let packed = match self.decimal_interner.entry(d.clone()) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => *e.insert(pack_old_idx(alloc_into(
                &mut self.objects,
                &mut self.free,
                &mut self.alloc_count,
                &mut self.gc_alloc_since_collect,
                HeapObj::Decimal(Box::new(d)),
            ))),
        };
        VmValue::from_heap_idx(packed)
    }
}
