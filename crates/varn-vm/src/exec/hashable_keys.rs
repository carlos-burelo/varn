use super::ctx::ExecCtx;
use crate::error::{RuntimeError, VmResult};
use crate::heap::HeapObj;
use crate::value::VmValue;

impl ExecCtx {
    pub(crate) fn hashable_key(&mut self, key: VmValue) -> VmResult<VmValue> {
        let Some(class_id) = self.instance_class_id(key) else {
            return Ok(key);
        };
        let (Some(hash_fn), Some(equals_fn)) = (
            self.bound_method(key, "hash"),
            self.bound_method(key, "equals"),
        ) else {
            return Ok(key);
        };
        let hash = self.invoke(hash_fn, &[hash_fn])?;
        if !hash.is_int() {
            return Err(RuntimeError::new("hash() must return int"));
        }
        let bucket_key = (class_id, hash.as_int());
        let candidates = unsafe { &*self.hashable_keys.get() }
            .get(&bucket_key)
            .cloned()
            .unwrap_or_default();
        for rep in candidates {
            if rep == key {
                return Ok(rep);
            }
            let same = self.invoke(equals_fn, &[equals_fn, rep])?;
            if same.is_bool() && same.as_bool() {
                return Ok(rep);
            }
        }
        unsafe { &mut *self.hashable_keys.get() }
            .entry(bucket_key)
            .or_default()
            .push(key);
        Ok(key)
    }

    fn instance_class_id(&self, v: VmValue) -> Option<u32> {
        if !v.is_heap() {
            return None;
        }
        match self.heap.get(v.as_heap()) {
            Some(HeapObj::Instance(inst)) => Some(inst.class_id),
            _ => None,
        }
    }

    pub(crate) fn bound_method(&mut self, recv: VmValue, name: &str) -> Option<VmValue> {
        let method = super::props::get_property(recv, name, &mut self.heap).ok()?;
        let callable = method.is_heap()
            && matches!(
                self.heap.get(method.as_heap()),
                Some(HeapObj::BoundMethod(_) | HeapObj::VmClosure(_) | HeapObj::NativeFn(..))
            );
        callable.then_some(method)
    }
}
