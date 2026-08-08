//! Map key canonicalization.
//!
//! Two values that are `==` must hash to the same key, which for heap values
//! means resolving them to something representation-independent before they
//! are used to index a Map or Set.

use crate::value::VmValue;
use varn_types::{
    value::MapKey, Value,
};
use super::obj::HeapObj;
use super::structs::HeapInner;

impl HeapInner {
    pub(crate) fn lookup_str_map_key(&self, s: &str) -> Option<MapKey> {
        if let Some(sso) = VmValue::try_from_sso(s) {
            return Some(MapKey(sso));
        }
        self.string_interner
            .get(s)
            .map(|&packed| MapKey(VmValue::from_heap_idx(packed)))
    }

    pub(crate) fn lookup_map_key(&self, v: VmValue) -> Option<MapKey> {
        if v.is_f64() {
            if v.as_f64() == 0.0 {
                return Some(MapKey(VmValue::from_f64(0.0)));
            }
            return Some(MapKey(v));
        }
        if !v.is_heap() {
            return Some(MapKey(v));
        }
        match self.get_by_idx(v.as_heap_idx()) {
            Some(HeapObj::Str(s)) => self.lookup_str_map_key(s.as_str()),
            Some(HeapObj::Char(c)) => self
                .char_interner
                .get(c)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            Some(HeapObj::BigInt(b)) => self
                .bigint_interner
                .get(b)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            Some(HeapObj::Decimal(d)) => self
                .decimal_interner
                .get(d)
                .map(|&p| MapKey(VmValue::from_heap_idx(p))),
            _ => Some(MapKey(v)),
        }
    }

    pub(crate) fn canonical_map_key(&mut self, v: VmValue) -> MapKey {
        if v.is_f64() {
            if v.as_f64() == 0.0 {
                return MapKey(VmValue::from_f64(0.0));
            }
            return MapKey(v);
        }
        if !v.is_heap() {
            return MapKey(v);
        }
        enum Canon {
            Str(String),
            Char(char),
            BigInt(i128),
            Decimal(rust_decimal::Decimal),
            Identity,
        }
        let canon = match self.get_by_idx(v.as_heap_idx()) {
            Some(HeapObj::Str(s)) => Canon::Str(s.as_str().to_owned()),
            Some(HeapObj::Char(c)) => Canon::Char(*c),
            Some(HeapObj::BigInt(b)) => Canon::BigInt(*b),
            Some(HeapObj::Decimal(d)) => Canon::Decimal(**d),
            _ => Canon::Identity,
        };
        match canon {
            Canon::Str(s) => MapKey(self.alloc_str_interned(s)),
            Canon::Char(c) => MapKey(self.intern(Value::Char(c))),
            Canon::BigInt(b) => MapKey(self.intern(Value::BigInt(Box::new(b)))),
            Canon::Decimal(d) => MapKey(self.intern(Value::Decimal(Box::new(d)))),
            Canon::Identity => MapKey(v),
        }
    }

}
