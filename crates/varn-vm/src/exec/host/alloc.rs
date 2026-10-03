use super::*;

impl ExecCtx {
    // Map keys MUST canonicalize through the content interner —
    // `alloc_str_dynamic` (and the trait default's `intern`) would mint a
    // fresh index per call and break key equality.
    pub(super) fn host_str_map_key(&mut self, s: &str) -> varn_types::value::MapKey {
        match VmValue::try_from_sso(s) {
            Some(v) => varn_types::value::MapKey(v),
            None => varn_types::value::MapKey(self.heap.alloc_str_interned(s)),
        }
    }
    pub(super) fn host_alloc_object_with_shape(
        &mut self,
        shape: &std::rc::Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        self.heap.alloc_object_with_shape(shape, values)
    }
    pub(super) fn host_alloc_bound_native(
        &mut self,
        receiver: VmValue,
        func: NativeFn,
        name: &'static str,
    ) -> VmValue {
        self.heap.alloc_bound_native(receiver, func, name)
    }
}
