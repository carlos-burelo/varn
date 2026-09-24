//! `Hashable & Equatable` instances as Map/Set keys (spec §25).
//!
//! A key is canonicalized to a representative the way `str` and `bigint`
//! keys are, so the table itself keeps comparing keys by identity: the first
//! instance seen with the same class and `hash()` that `equals` the new one
//! stands for it. Only the context can run `hash()`/`equals()`, so the
//! representatives live here (rooted in `ExecCtx::major_roots` and the minor
//! collection), not in the heap's interners.

use super::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::NativeCtx;

impl ExecCtx {
    /// The representative `key` stands for, or `key` itself when it is not an
    /// instance declaring both `hash()` and `equals()`. A capability method
    /// that throws leaves the key as itself.
    pub(crate) fn hashable_key(&mut self, key: VmValue) -> VmValue {
        let Some(class_id) = self.instance_class_id(key) else {
            return key;
        };
        let (Some(hash_fn), Some(equals_fn)) =
            (self.bound_method(key, "hash"), self.bound_method(key, "equals"))
        else {
            return key;
        };
        let Ok(hash) = self.call_vm(hash_fn, &[]) else {
            return key;
        };
        let bucket_key = (class_id, self.heap.as_int(hash));
        let candidates = self.hashable_keys.get(&bucket_key).cloned().unwrap_or_default();
        for rep in candidates {
            if rep == key {
                return rep;
            }
            if let Ok(same) = self.call_vm(equals_fn, &[rep]) {
                if same.is_bool() && same.as_bool() {
                    return rep;
                }
            }
        }
        self.hashable_keys.entry(bucket_key).or_default().push(key);
        key
    }

    fn instance_class_id(&self, v: VmValue) -> Option<u32> {
        if !v.is_heap() {
            return None;
        }
        match self.heap.get(v.as_heap_idx()) {
            Some(HeapObj::Instance(inst)) => Some(inst.class_id),
            _ => None,
        }
    }

    fn bound_method(&mut self, recv: VmValue, name: &str) -> Option<VmValue> {
        let method = super::props::get_property(recv, name, &mut self.heap).ok()?;
        let callable = method.is_heap()
            && matches!(
                self.heap.get(method.as_heap_idx()),
                Some(HeapObj::BoundMethod(_) | HeapObj::VmClosure(_) | HeapObj::NativeFn(..))
            );
        callable.then_some(method)
    }
}
