use super::obj::HeapObj;
use super::structs::Heap;
use crate::value::VmValue;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::Arc;
use varn_types::{value::MapKey, ClassObj, NativeCtx, NativeFn, ResourceStore};

impl NativeCtx for Heap {
    fn int_val(&mut self, n: i64) -> VmValue {
        self.deref_mut().make_int(n)
    }

    fn is_int(&self, v: VmValue) -> bool {
        self.deref().is_int(v)
    }

    fn as_int(&self, v: VmValue) -> i64 {
        self.deref().as_int(v)
    }

    fn to_f64(&self, v: VmValue) -> f64 {
        self.deref().to_f64_val(v)
    }

    fn alloc_str(&mut self, s: &str) -> VmValue {
        self.deref_mut().alloc_str_dynamic(s)
    }

    fn alloc_str_owned(&mut self, s: String) -> VmValue {
        self.deref_mut().alloc_str_dynamic(&s)
    }

    fn alloc_bound_native(
        &mut self,
        receiver: VmValue,
        func: NativeFn,
        name: &'static str,
    ) -> VmValue {
        self.deref_mut().alloc_bound_native(receiver, func, name)
    }

    fn alloc_map(&mut self, entries: Vec<(VmValue, VmValue)>) -> VmValue {
        let mut map = varn_types::value::ValueMap::default();
        for (key, value) in entries {
            let key = self.deref_mut().canonical_map_key(key);
            map.insert(key, value);
        }
        self.deref_mut().alloc_map_vm(map)
    }

    fn alloc_set(&mut self, items: Vec<VmValue>) -> VmValue {
        let mut set = varn_types::value::ValueSet::default();
        for item in items {
            set.insert(self.deref_mut().canonical_map_key(item));
        }
        self.deref_mut().alloc_set_vm(set)
    }

    fn alloc_enum_variant(&mut self, data: varn_types::value::EnumVariantData) -> VmValue {
        self.deref_mut().alloc_enum_variant_vm(data)
    }

    fn alloc_buffer(&mut self, size: usize) -> VmValue {
        self.deref_mut()
            .alloc_vm_buffer(varn_types::VmBuffer::new(size))
    }

    fn alloc_buffer_from_bytes(&mut self, bytes: &[u8]) -> VmValue {
        self.deref_mut()
            .alloc_vm_buffer(varn_types::VmBuffer::from_bytes(bytes))
    }

    fn alloc_array(&mut self, items: Vec<VmValue>) -> VmValue {
        self.alloc_array_vm(items)
    }

    fn alloc_object(&mut self) -> VmValue {
        self.deref_mut().alloc_object()
    }

    fn alloc_object_with_shape(
        &mut self,
        shape: &std::rc::Rc<varn_types::Shape>,
        values: Vec<VmValue>,
    ) -> VmValue {
        self.deref_mut().alloc_object_with_shape(shape, values)
    }

    fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue {
        self.deref_mut().alloc_range(start, end, inclusive)
    }

    fn alloc_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue {
        self.alloc_native_fn(f, name)
    }

    fn alloc_class(&mut self, class: Rc<ClassObj>) -> VmValue {
        self.deref_mut().alloc_class_vm(class)
    }

    fn is_string(&self, v: VmValue) -> bool {
        self.deref().is_string(v)
    }

    fn is_array(&self, v: VmValue) -> bool {
        v.is_heap() && matches!(self.get(v.as_heap()), Some(HeapObj::Array(_)))
    }

    fn str_repr(&self, v: VmValue) -> String {
        self.deref().str_repr(v)
    }

    fn str_repr_borrowed<'a>(&'a self, v: VmValue) -> std::borrow::Cow<'a, str> {
        self.deref().str_repr_borrowed(v)
    }

    fn str_owned(&self, v: VmValue) -> Option<String> {
        self.deref().str_owned(v)
    }

    fn str_shared(&self, v: VmValue) -> Option<std::sync::Arc<str>> {
        if v.is_sso() {
            let mut buf = [0u8; 5];
            return Some(std::sync::Arc::from(v.sso_as_str(&mut buf)));
        }
        if v.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get(v.as_heap()) {
                return Some(s.to_shared());
            }
        }
        None
    }

    fn str_is_ascii(&self, v: VmValue) -> bool {
        if v.is_sso() {
            let mut buf = [0u8; 5];
            return v.sso_as_str(&mut buf).is_ascii();
        }
        if v.is_heap() {
            if let Some(HeapObj::Str(s)) = self.get(v.as_heap()) {
                
                
                return s.is_ascii();
            }
        }
        false
    }

    fn array_len(&self, arr: VmValue) -> usize {
        crate::heap_array::array_len(self, arr)
    }

    fn array_get(&self, arr: VmValue, idx: usize) -> Option<VmValue> {
        crate::heap_array::array_get(self, arr, idx)
    }

    fn array_set(&mut self, arr: VmValue, idx: usize, val: VmValue) {
        crate::heap_array::array_set(self, arr, idx, val)
    }

    fn array_push(&mut self, arr: VmValue, val: VmValue) {
        crate::heap_array::array_push(self, arr, val)
    }

    fn array_pop(&mut self, arr: VmValue) -> Option<VmValue> {
        crate::heap_array::array_pop(self, arr)
    }

    fn array_for_each(&self, arr: VmValue, f: &mut dyn FnMut(VmValue, usize)) {
        crate::heap_array::array_for_each(self, arr, f)
    }

    fn is_object(&self, v: VmValue) -> bool {
        if v.is_heap() {
            matches!(self.get(v.as_heap()), Some(HeapObj::Object(_)))
        } else {
            false
        }
    }

    fn object_for_each(&self, obj: VmValue, f: &mut dyn FnMut(&str, VmValue)) {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.get(obj.as_heap()) {
                let g = o.borrow();
                for (k, v) in g.iter() {
                    f(k.as_ref(), v);
                }
            }
        }
    }

    fn get_object_shape(&self, obj: VmValue) -> Option<std::rc::Rc<varn_types::Shape>> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.get(obj.as_heap()) {
                return Some(Rc::clone(o.borrow().shape()));
            }
        }
        None
    }

    fn get_field(&self, obj: VmValue, key: &str) -> Option<VmValue> {
        if obj.is_heap() {
            if let Some(HeapObj::Object(o)) = self.get(obj.as_heap()) {
                return o.borrow().get_field(key);
            }
            if let Some(HeapObj::Module(m)) = self.get(obj.as_heap()) {
                let slot = m.export_map.get(key).copied()?;
                return m.get_slot(slot);
            }
        }
        None
    }

    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue) {
        if obj.is_heap() {
            let raw_idx = obj.as_heap();
            if let Some(HeapObj::Object(o)) = self.get(raw_idx) {
                o.set_field(Arc::from(key), val);
                self.write_barrier(raw_idx, val);
            } else if let Some(HeapObj::Module(m)) = self.get_mut(raw_idx) {
                if let Some(s) = m.export_map.get(key).copied() {
                    Rc::make_mut(m).set_slot(s, val);
                } else {
                    let m = Rc::make_mut(m);
                    let slot = m.exports.len();
                    m.exports.push(val);
                    m.export_map.insert(Arc::from(key), slot);
                }
                self.write_barrier(raw_idx, val);
            }
        }
    }

    fn finalize(&mut self, obj: VmValue) -> VmValue {
        obj
    }

    fn call_vm(
        &mut self,
        _callee: VmValue,
        _args: &[VmValue],
    ) -> Result<VmValue, varn_types::NativeError> {
        Err("call_vm unavailable on bare Heap (use ExecCtx)".into())
    }

    fn spawn_vm(&mut self, _callee: VmValue, _args: &[VmValue]) -> Result<VmValue, String> {
        Err("spawn_vm unavailable on bare Heap (use ExecCtx)".into())
    }

    fn suspend_timer(&mut self, _ms: u64) -> VmValue {
        VmValue::null()
    }

    fn resources(&mut self) -> &mut ResourceStore {
        panic!("resources() unavailable on bare Heap")
    }

    fn as_generator(&self, v: VmValue) -> Option<varn_types::GeneratorObj> {
        self.deref().generator_of(v)
    }

    fn as_char(&self, v: VmValue) -> Option<char> {
        self.deref().char_of(v)
    }

    fn as_bigint(&self, v: VmValue) -> Option<num_bigint::BigInt> {
        self.deref().bigint_of(v)
    }

    fn as_decimal(&self, v: VmValue) -> Option<bigdecimal::BigDecimal> {
        self.deref().decimal_of(v)
    }

    fn as_range(&self, v: VmValue) -> Option<varn_types::value::RangeData> {
        self.deref().range_of(v)
    }

    fn as_map(&self, v: VmValue) -> Option<varn_types::value::MapRef> {
        self.deref().map_of(v)
    }

    fn as_set(&self, v: VmValue) -> Option<varn_types::value::SetRef> {
        self.deref().set_of(v)
    }

    fn is_static_receiver(&self, v: VmValue) -> bool {
        self.deref().is_static_receiver(v)
    }

    fn alloc_range_data(&mut self, range: varn_types::value::RangeData) -> VmValue {
        self.deref_mut().alloc_range_data(range)
    }

    fn alloc_char(&mut self, value: char) -> VmValue {
        self.deref_mut().alloc_char(value)
    }

    fn alloc_bigint(&mut self, value: num_bigint::BigInt) -> VmValue {
        self.deref_mut().alloc_bigint(value)
    }

    fn alloc_decimal(&mut self, value: bigdecimal::BigDecimal) -> VmValue {
        self.deref_mut().alloc_decimal(value)
    }

    fn map_key(&mut self, v: VmValue) -> Result<MapKey, varn_types::NativeError> {
        Ok(self.deref_mut().canonical_map_key(v))
    }

    fn str_map_key(&mut self, s: &str) -> MapKey {
        match VmValue::try_from_sso(s) {
            Some(v) => MapKey(v),
            None => MapKey(self.deref_mut().alloc_str_interned(s)),
        }
    }

    fn collection_write_barrier(&mut self, parent: VmValue, child: VmValue) {
        if parent.is_heap() {
            self.deref_mut().write_barrier(parent.as_heap(), child);
        }
    }

    fn call_static(&mut self, f: NativeFn) -> VmValue {
        varn_types::call_static_with(f)
    }

    fn get_class(&self, name: &str) -> Option<std::rc::Rc<ClassObj>> {
        self.get_intrinsic_class(name)
    }

    fn register_class(&mut self, name: &str, cls: std::rc::Rc<ClassObj>) {
        self.set_intrinsic_class(name, cls);
    }
}
