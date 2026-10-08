use super::obj::HeapObj;
use super::structs::HeapInner;
use crate::value::VmValue;
use varn_types::value::MapKey;

impl HeapInner {
    pub(crate) fn lookup_str_map_key(&self, s: &str) -> Option<MapKey> {
        if let Some(sso) = VmValue::try_from_sso(s) {
            return Some(MapKey(sso));
        }
        self.string_interner
            .get(s)
            .map(|&packed| MapKey(VmValue::from_heap(packed)))
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
        match self.get(v.as_heap()) {
            Some(HeapObj::Str(s)) => self.lookup_str_map_key(s.as_str()),
            Some(HeapObj::Char(c)) => self
                .char_interner
                .get(c)
                .map(|&p| MapKey(VmValue::from_heap(p))),
            Some(HeapObj::BigInt(b)) => self
                .bigint_interner
                .get(b)
                .map(|&p| MapKey(VmValue::from_heap(p))),
            Some(HeapObj::Decimal(d)) => self
                .decimal_interner
                .get(d)
                .map(|&p| MapKey(VmValue::from_heap(p))),
            Some(HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::VmClosure(_) | HeapObj::Class(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::Task(_) | HeapObj::TaskHandle(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::Generator(_) | HeapObj::Spread(_)) | None => Some(MapKey(v)),
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
        let str_action = match self.get(v.as_heap()) {
            Some(HeapObj::Str(s)) => {
                if let Some(k) = self.lookup_str_map_key(s.as_str()) {
                    return k;
                }
                Some(s.as_str().to_owned())
            }
            Some(HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::VmClosure(_) | HeapObj::Class(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::Task(_) | HeapObj::TaskHandle(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::BigInt(_) | HeapObj::Decimal(_) | HeapObj::Char(_) | HeapObj::Generator(_) | HeapObj::Spread(_)) | None => None,
        };
        if let Some(s_owned) = str_action {
            return MapKey(self.alloc_str_interned(s_owned));
        }
        match self.get(v.as_heap()) {
            Some(HeapObj::Char(c)) => {
                let c = *c;
                MapKey(self.alloc_char(c))
            }
            Some(HeapObj::BigInt(b)) => {
                let b = b.clone();
                MapKey(self.alloc_bigint(*b))
            }
            Some(HeapObj::Decimal(d)) => {
                let d = d.clone();
                MapKey(self.intern_decimal(*d))
            }
            Some(HeapObj::Str(_) | HeapObj::Array(_) | HeapObj::Tuple(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Buffer(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) | HeapObj::VmClosure(_) | HeapObj::Class(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) | HeapObj::Map(_) | HeapObj::Set(_) | HeapObj::Task(_) | HeapObj::TaskHandle(_) | HeapObj::Range(_) | HeapObj::Symbol(_) | HeapObj::EnumVariant(_) | HeapObj::Generator(_) | HeapObj::Spread(_)) | None => MapKey(v),
        }
    }
}
