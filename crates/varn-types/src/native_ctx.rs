use std::borrow::Cow;
use std::rc::Rc;

use crate::native::NativeFn;
use crate::resource::ResourceStore;
use crate::value::SendValue;
use crate::vm_value::VmValue;
use crate::ClassObj;

pub trait NativeCtx {
    fn null_val(&self) -> VmValue {
        VmValue::null()
    }
    fn bool_val(&self, b: bool) -> VmValue {
        VmValue::from_bool(b)
    }
    fn int_val(&self, n: i64) -> VmValue {
        VmValue::from_int(n)
    }

    fn alloc_str(&mut self, s: &str) -> VmValue;
    fn alloc_str_owned(&mut self, s: String) -> VmValue;
    fn alloc_array(&mut self, items: Vec<VmValue>) -> VmValue;
    fn alloc_object(&mut self) -> VmValue;
    fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue;
    fn alloc_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue;
    fn alloc_class(&mut self, class: Rc<ClassObj>) -> VmValue;

    fn is_string(&self, v: VmValue) -> bool;
    fn is_array(&self, v: VmValue) -> bool;
    fn is_null(&self, v: VmValue) -> bool {
        v.is_null()
    }
    fn str_repr(&self, v: VmValue) -> String;
    fn str_repr_borrowed<'a>(&'a self, v: VmValue) -> Cow<'a, str> {
        Cow::Owned(self.str_repr(v))
    }
    fn str_owned(&self, v: VmValue) -> Option<String>;

    fn array_len(&self, arr: VmValue) -> usize;
    fn array_get(&self, arr: VmValue, idx: usize) -> Option<VmValue>;
    fn array_set(&mut self, arr: VmValue, idx: usize, val: VmValue);
    fn array_push(&mut self, arr: VmValue, val: VmValue);
    fn array_pop(&mut self, arr: VmValue) -> Option<VmValue>;
    fn array_for_each(&self, arr: VmValue, f: &mut dyn FnMut(VmValue, usize));

    fn get_field(&self, obj: VmValue, key: &str) -> Option<VmValue>;
    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue);

    fn finalize(&mut self, obj: VmValue) -> VmValue {
        obj
    }

    fn call_vm(&mut self, callee: VmValue, args: &[VmValue]) -> Result<VmValue, String>;
    fn spawn_vm(&mut self, callee: VmValue, args: &[VmValue]) -> Result<VmValue, String>;

    fn set_timer(
        &mut self,
        ms: u64,
        repeat: bool,
        callee: VmValue,
        args: &[VmValue],
    ) -> Result<usize, String>;
    fn clear_timer(&mut self, id: usize) -> Result<(), String>;
    fn suspend_timer(&mut self, ms: u64) -> VmValue;

    fn has_capability(&self, _cap: &str) -> bool {
        true
    }

    fn resources(&mut self) -> &mut ResourceStore;

    fn extract(&self, v: VmValue) -> crate::Value;
    fn intern(&mut self, v: crate::Value) -> VmValue;

    fn intern_value(&mut self, v: crate::Value) -> VmValue {
        self.intern(v)
    }

    fn alloc_obj(&mut self) -> VmValue {
        self.alloc_object()
    }

    fn call_static(&mut self, f: NativeFn) -> VmValue;

    fn get_class(&self, _name: &str) -> Option<std::rc::Rc<ClassObj>> {
        None
    }

    fn register_class(&mut self, _name: &str, _cls: std::rc::Rc<ClassObj>) {}

    fn current_source_file(&self) -> Option<String> {
        None
    }

    fn spawn_isolate(
        &mut self,
        _module_path: &str,
        _export_name: &str,
        _args: Vec<crate::value::SendValue>,
        _port: Box<dyn varn_base::VmValuePayload + Send + Sync>,
    ) -> Result<(), String> {
        Err("spawn_isolate not supported".to_string())
    }

    fn alloc_instance(&mut self, _class_name: &str) -> Option<VmValue> {
        None
    }

    fn get_function_location(&self, _func_val: VmValue) -> Option<(String, String)> {
        None
    }

    fn load_module(&mut self, _specifier: &str) -> Result<VmValue, String> {
        Err("load_module not supported".to_string())
    }

    fn to_sendable(&self, _val: VmValue) -> Result<SendValue, String> {
        Err("to_sendable not supported".to_string())
    }
}

pub type NativeFnResult = Result<VmValue, String>;
