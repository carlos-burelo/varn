use super::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;

const TUPLE_BUCKET: u32 = u32::MAX;
const ENUM_BUCKET: u32 = u32::MAX - 1;

fn same_value_zero(n: f64) -> VmValue {
    if n.is_nan() {
        VmValue::from_f64(f64::NAN)
    } else if n == 0.0 {
        VmValue::from_f64(0.0)
    } else {
        VmValue::from_f64(n)
    }
}

impl ExecCtx {
    pub(crate) fn canonical_key(&mut self, key: VmValue) -> VmValue {
        if key.is_f64() {
            return same_value_zero(key.as_f64());
        }
        match self.instance_class_id(key) {
            Some(class_id) => self.value_instance_key(key, class_id),
            None => match self.composite_bucket(key) {
                Some(bucket) => self.structural_key(key, bucket),
                None => key,
            },
        }
    }

    fn value_instance_key(&mut self, key: VmValue, class_id: u32) -> VmValue {
        let is_value =
            varn_types::ClassObj::find_by_id(class_id).is_some_and(|class| class.layout().is_value);
        if !is_value {
            return key;
        }
        self.structural_key(key, class_id)
    }

    fn composite_bucket(&self, v: VmValue) -> Option<u32> {
        if !v.is_heap() {
            return None;
        }
        match self.heap.get(v.as_heap()) {
            Some(HeapObj::Tuple(_)) => Some(TUPLE_BUCKET),
            Some(HeapObj::EnumVariant(_)) => Some(ENUM_BUCKET),
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Object(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Range(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_)
                | HeapObj::Char(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_),
            )
            | None => None,
        }
    }

    fn structural_key(&mut self, key: VmValue, bucket: u32) -> VmValue {
        let bucket_key = (bucket, super::value_hash::structural_hash(key, &self.heap));
        let candidates = unsafe { &*self.hashable_keys.get() }
            .get(&bucket_key)
            .cloned()
            .unwrap_or_default();
        for rep in candidates {
            if rep == key || super::compare::eq(rep, key, &self.heap) {
                return rep;
            }
        }
        unsafe { &mut *self.hashable_keys.get() }
            .entry(bucket_key)
            .or_default()
            .push(key);
        key
    }

    fn instance_class_id(&self, v: VmValue) -> Option<u32> {
        if !v.is_heap() {
            return None;
        }
        self.heap.instance(v.as_heap()).map(|inst| inst.class_id)
    }
}
