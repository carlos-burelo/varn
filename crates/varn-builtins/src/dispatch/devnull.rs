use varn_types::VmValue;

pub struct DevNullModuleCtx;

impl varn_types::NativeCtx for DevNullModuleCtx {
    fn alloc_bigint(&mut self, _value: num_bigint::BigInt) -> VmValue {
        VmValue::null()
    }
    fn alloc_decimal(&mut self, _value: bigdecimal::BigDecimal) -> VmValue {
        VmValue::null()
    }
    fn alloc_char(&mut self, _value: char) -> VmValue {
        VmValue::null()
    }
    fn alloc_map(&mut self, _entries: Vec<(VmValue, VmValue)>) -> VmValue {
        VmValue::null()
    }
    fn alloc_set(&mut self, _items: Vec<VmValue>) -> VmValue {
        VmValue::null()
    }
    fn alloc_enum_variant(&mut self, _data: varn_types::value::EnumVariantData) -> VmValue {
        VmValue::null()
    }
    fn alloc_bound_native(
        &mut self,
        _receiver: VmValue,
        _func: varn_types::NativeFn,
        _name: &'static str,
    ) -> VmValue {
        VmValue::null()
    }
    fn alloc_buffer(&mut self, _size: usize) -> VmValue {
        VmValue::null()
    }
    fn alloc_buffer_from_bytes(&mut self, _bytes: &[u8]) -> VmValue {
        VmValue::null()
    }
    fn alloc_range_data(&mut self, _range: varn_types::value::RangeData) -> VmValue {
        VmValue::null()
    }
    fn alloc_str(&mut self, _s: &str) -> VmValue {
        VmValue::null()
    }
    fn alloc_str_owned(&mut self, _s: String) -> VmValue {
        VmValue::null()
    }
    fn alloc_array(&mut self, _items: Vec<VmValue>) -> VmValue {
        VmValue::null()
    }
    fn alloc_object(&mut self) -> VmValue {
        VmValue::null()
    }
    fn alloc_range(&mut self, _s: i64, _e: i64, _i: bool) -> VmValue {
        VmValue::null()
    }
    fn alloc_fn(&mut self, _f: varn_types::NativeFn, _name: &'static str) -> VmValue {
        VmValue::null()
    }
    fn alloc_class(&mut self, _c: std::rc::Rc<varn_types::ClassObj>) -> VmValue {
        VmValue::null()
    }
    fn is_string(&self, _v: VmValue) -> bool {
        false
    }
    fn is_array(&self, _v: VmValue) -> bool {
        false
    }
    fn str_repr(&self, _v: VmValue) -> String {
        String::new()
    }
    fn str_owned(&self, _v: VmValue) -> Option<String> {
        None
    }
    fn array_len(&self, _arr: VmValue) -> usize {
        0
    }
    fn array_get(&self, _arr: VmValue, _idx: usize) -> Option<VmValue> {
        None
    }
    fn array_set(&mut self, _arr: VmValue, _idx: usize, _val: VmValue) {}
    fn array_push(&mut self, _arr: VmValue, _val: VmValue) {}
    fn array_pop(&mut self, _arr: VmValue) -> Option<VmValue> {
        None
    }
    fn array_for_each(&self, _arr: VmValue, _f: &mut dyn FnMut(VmValue, usize)) {}
    fn get_field(&self, _obj: VmValue, _key: &str) -> Option<VmValue> {
        None
    }
    fn set_field(&mut self, _obj: VmValue, _key: &str, _val: VmValue) {}
    fn call_vm(&mut self, _c: VmValue, _a: &[VmValue]) -> Result<VmValue, varn_types::NativeError> {
        Ok(VmValue::null())
    }
    fn spawn_vm(&mut self, _c: VmValue, _a: &[VmValue]) -> Result<VmValue, String> {
        Ok(VmValue::null())
    }
    fn suspend_timer(&mut self, _ms: u64) -> VmValue {
        VmValue::null()
    }
    fn resources(&mut self) -> &mut varn_types::ResourceStore {
        panic!("DevNullModuleCtx::resources")
    }
    fn call_static(&mut self, _f: varn_types::NativeFn) -> VmValue {
        VmValue::null()
    }
}
