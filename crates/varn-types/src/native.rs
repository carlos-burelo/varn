use std::rc::Rc;

use crate::{native_ctx::NativeCtx, vm_value::VmValue};

pub type NativeFn = fn(&mut dyn NativeCtx, &[VmValue]) -> crate::NativeFnResult;

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Function = 0x01,
    ClassConstructor = 0x02,
    InstanceMethod = 0x03,
    StaticMethod = 0x04,
    Getter = 0x05,
    Setter = 0x06,
    PrimitiveExt = 0x07,
    EnumVariant = 0x08,
    StaticValue = 0x09,
    ClassDef = 0x10,
    Constructor = 0x11,
    InstanceGetter = 0x12,
    InstanceSetter = 0x13,
    StaticGetter = 0x14,
    StaticSetter = 0x15,
    ExtMethod = 0x16,
    ExtGetter = 0x17,
    EnumDef = 0x18,
    ConstValue = 0x19,
    AsyncFunction = 0x1A,
    NamespaceDef = 0x1B,
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

#[repr(C, align(16))]
pub struct NativeOpEntry {
    pub module_id: *const u8,
    pub module_id_len: u32,

    pub namespace_path: *const u8,
    pub namespace_path_len: u32,

    pub symbol_name: *const u8,
    pub symbol_name_len: u32,

    pub func_ptr: *const u8,
    pub raw_func_ptr: *const u8,
    pub signature: SignatureDescriptor,
    pub capability_mask: u64,

    pub entry_kind: u8,
    pub flags: u32,
    pub _reserved: [u8; 7],
}

unsafe impl Sync for NativeOpEntry {}
unsafe impl Send for NativeOpEntry {}

impl NativeOpEntry {
    pub fn module_id(&self) -> &str {
        unsafe {
            let slice = std::slice::from_raw_parts(self.module_id, self.module_id_len as usize);
            std::str::from_utf8_unchecked(slice)
        }
    }

    pub fn namespace_path(&self) -> &str {
        unsafe {
            let slice =
                std::slice::from_raw_parts(self.namespace_path, self.namespace_path_len as usize);
            std::str::from_utf8_unchecked(slice)
        }
    }

    pub fn symbol_name(&self) -> &str {
        unsafe {
            let slice = std::slice::from_raw_parts(self.symbol_name, self.symbol_name_len as usize);
            std::str::from_utf8_unchecked(slice)
        }
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
    fn alloc_buffer(&mut self, _size: usize) -> VmValue {
        VmValue::null()
    }
    fn alloc_buffer_from_bytes(&mut self, _bytes: &[u8]) -> VmValue {
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
    fn alloc_buffer(&mut self, _size: usize) -> VmValue {
        VmValue::null()
    }
    fn alloc_buffer_from_bytes(&mut self, _bytes: &[u8]) -> VmValue {
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

pub fn call_static_with(f: NativeFn) -> VmValue {
    (f)(&mut StaticInitCtx, &[]).unwrap_or(VmValue::null())
}
