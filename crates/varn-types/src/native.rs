use std::rc::Rc;

use crate::{native_ctx::NativeCtx, vm_value::VmValue};

pub type NativeFn = fn(&mut dyn NativeCtx, &[VmValue]) -> crate::NativeFnResult;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Function,
    ClassDef,
    InstanceMethod,
    StaticMethod,
    InstanceGetter,
    StaticGetter,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ArgType {
    Void = 0,
    Int = 1,
    Float = 2,
    Bool = 3,
    Char = 4,
    Str = 5,
    Context = 6,
    Generic = 7,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SignatureDescriptor {
    pub return_type: ArgType,
    pub param_count: u8,
    pub param_types: [ArgType; 7],
}

impl SignatureDescriptor {
    pub const fn empty() -> Self {
        Self {
            return_type: ArgType::Void,
            param_count: 0,
            param_types: [ArgType::Void; 7],
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct NativeOpTarget {
    pub func_ptr: usize,

    pub raw_func_ptr: usize,

    pub signature: SignatureDescriptor,
}

impl NativeOpTarget {
    pub const fn unknown() -> Self {
        Self {
            func_ptr: 0,
            raw_func_ptr: 0,
            signature: SignatureDescriptor::empty(),
        }
    }
}

pub struct NativeOpEntry {
    pub module_id: &'static str,
    pub namespace_path: &'static str,
    pub symbol_name: &'static str,

    pub func_ptr: *const u8,
    pub raw_func_ptr: *const u8,
    pub signature: SignatureDescriptor,

    pub entry_kind: EntryKind,
    pub flags: u32,
    pub capability: &'static str,
}

unsafe impl Sync for NativeOpEntry {}
unsafe impl Send for NativeOpEntry {}

impl NativeOpEntry {
    pub fn module_id(&self) -> &'static str {
        self.module_id
    }

    pub fn namespace_path(&self) -> &'static str {
        self.namespace_path
    }

    pub fn symbol_name(&self) -> &'static str {
        self.symbol_name
    }

    pub fn capability(&self) -> Option<&'static str> {
        (!self.capability.is_empty()).then_some(self.capability)
    }

    pub fn func(&self) -> NativeFn {
        unsafe { std::mem::transmute(self.func_ptr) }
    }
}

use crate::resource::ResourceStore;

pub struct DummyCtx;

impl NativeCtx for DummyCtx {
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
    fn alloc_enum_variant(&mut self, _data: crate::value::EnumVariantData) -> VmValue {
        VmValue::null()
    }
    fn alloc_bound_native(
        &mut self,
        _receiver: VmValue,
        _func: NativeFn,
        _name: &'static str,
    ) -> VmValue {
        VmValue::null()
    }
    fn alloc_range_data(&mut self, _range: crate::value::RangeData) -> VmValue {
        VmValue::null()
    }
    fn alloc_str(&mut self, _s: &str) -> VmValue {
        VmValue::null()
    }
    fn alloc_str_owned(&mut self, _s: String) -> VmValue {
        VmValue::null()
    }
    fn str_repr(&self, _v: VmValue) -> String {
        String::new()
    }
    fn str_owned(&self, _v: VmValue) -> Option<String> {
        None
    }
    fn is_string(&self, _v: VmValue) -> bool {
        false
    }
    fn is_array(&self, _v: VmValue) -> bool {
        false
    }
    fn alloc_array(&mut self, _items: Vec<VmValue>) -> VmValue {
        VmValue::null()
    }
    fn alloc_tuple(&mut self, _items: Vec<VmValue>) -> VmValue {
        VmValue::null()
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
    fn alloc_object(&mut self) -> VmValue {
        VmValue::null()
    }
    fn get_field(&self, _obj: VmValue, _key: &str) -> Option<VmValue> {
        None
    }
    fn set_field(&mut self, _obj: VmValue, _key: &str, _val: VmValue) {}
    fn alloc_fn(&mut self, _f: NativeFn, _name: &'static str) -> VmValue {
        VmValue::null()
    }
    fn alloc_class(&mut self, _class: Rc<crate::ClassObj>) -> VmValue {
        VmValue::null()
    }
    fn alloc_range(&mut self, _start: i64, _end: i64, _inclusive: bool) -> VmValue {
        VmValue::null()
    }
    fn call_vm(
        &mut self,
        _callee: VmValue,
        _args: &[VmValue],
    ) -> Result<VmValue, crate::NativeError> {
        Err("DummyCtx".into())
    }
    fn spawn_vm(&mut self, _callee: VmValue, _args: &[VmValue]) -> Result<VmValue, String> {
        Err("DummyCtx".into())
    }
    fn suspend_timer(&mut self, _ms: u64) -> VmValue {
        VmValue::null()
    }
    fn resources(&mut self) -> &mut ResourceStore {
        panic!("DummyCtx")
    }
    fn call_static(&mut self, _f: NativeFn) -> VmValue {
        VmValue::null()
    }
}

impl crate::Buffers for DummyCtx {
    fn alloc_buffer(&mut self, _size: usize) -> VmValue {
        VmValue::null()
    }

    fn alloc_buffer_from_bytes(&mut self, _bytes: &[u8]) -> VmValue {
        VmValue::null()
    }
}

struct StaticInitCtx;

impl NativeCtx for StaticInitCtx {
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
    fn alloc_enum_variant(&mut self, _data: crate::value::EnumVariantData) -> VmValue {
        VmValue::null()
    }
    fn alloc_bound_native(
        &mut self,
        _receiver: VmValue,
        _func: NativeFn,
        _name: &'static str,
    ) -> VmValue {
        VmValue::null()
    }
    fn alloc_range_data(&mut self, _range: crate::value::RangeData) -> VmValue {
        VmValue::null()
    }
    fn alloc_str(&mut self, _s: &str) -> VmValue {
        VmValue::null()
    }
    fn alloc_str_owned(&mut self, _s: String) -> VmValue {
        VmValue::null()
    }
    fn str_repr(&self, _v: VmValue) -> String {
        String::new()
    }
    fn str_owned(&self, _v: VmValue) -> Option<String> {
        None
    }
    fn is_string(&self, _v: VmValue) -> bool {
        false
    }
    fn is_array(&self, _v: VmValue) -> bool {
        false
    }
    fn alloc_array(&mut self, _items: Vec<VmValue>) -> VmValue {
        VmValue::null()
    }
    fn alloc_tuple(&mut self, _items: Vec<VmValue>) -> VmValue {
        VmValue::null()
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
    fn alloc_object(&mut self) -> VmValue {
        VmValue::null()
    }
    fn get_field(&self, _obj: VmValue, _key: &str) -> Option<VmValue> {
        None
    }
    fn set_field(&mut self, _obj: VmValue, _key: &str, _val: VmValue) {}
    fn alloc_fn(&mut self, _f: NativeFn, _name: &'static str) -> VmValue {
        VmValue::null()
    }
    fn alloc_class(&mut self, _class: Rc<crate::ClassObj>) -> VmValue {
        VmValue::null()
    }
    fn alloc_range(&mut self, _start: i64, _end: i64, _inclusive: bool) -> VmValue {
        VmValue::null()
    }
    fn call_vm(
        &mut self,
        _callee: VmValue,
        _args: &[VmValue],
    ) -> Result<VmValue, crate::NativeError> {
        Err("call_vm unavailable in static init context".into())
    }
    fn spawn_vm(&mut self, _callee: VmValue, _args: &[VmValue]) -> Result<VmValue, String> {
        Err("spawn_vm unavailable in static init context".into())
    }
    fn suspend_timer(&mut self, _ms: u64) -> VmValue {
        VmValue::null()
    }
    fn resources(&mut self) -> &mut ResourceStore {
        panic!("resources() unavailable in static init context")
    }
    fn call_static(&mut self, f: NativeFn) -> VmValue {
        (f)(self as &mut dyn NativeCtx, &[]).unwrap_or(VmValue::null())
    }
}

impl crate::Buffers for StaticInitCtx {
    fn alloc_buffer(&mut self, _size: usize) -> VmValue {
        VmValue::null()
    }

    fn alloc_buffer_from_bytes(&mut self, _bytes: &[u8]) -> VmValue {
        VmValue::null()
    }
}

pub fn call_static_with(f: NativeFn) -> VmValue {
    (f)(&mut StaticInitCtx, &[]).unwrap_or(VmValue::null())
}
