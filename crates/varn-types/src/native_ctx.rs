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
    fn int_val(&mut self, n: i64) -> VmValue {
        self.intern(crate::Value::Int(n))
    }

    fn is_int(&self, v: VmValue) -> bool {
        v.is_int() || (v.is_heap() && matches!(self.extract(v), crate::Value::Int(_)))
    }

    fn as_int(&self, v: VmValue) -> i64 {
        if v.is_int() {
            v.as_int()
        } else {
            match self.extract(v) {
                crate::Value::Int(n) => n,
                _ => 0,
            }
        }
    }

    fn to_f64(&self, v: VmValue) -> f64 {
        if v.is_f64() {
            v.as_f64()
        } else if v.is_int() {
            v.as_int() as f64
        } else {
            match self.extract(v) {
                crate::Value::Int(n) => n as f64,
                crate::Value::Float(f) => f,
                _ => 0.0,
            }
        }
    }

    fn alloc_str(&mut self, s: &str) -> VmValue;
    fn alloc_str_owned(&mut self, s: String) -> VmValue;
    fn alloc_array(&mut self, items: Vec<VmValue>) -> VmValue;
    fn alloc_object(&mut self) -> VmValue;
    fn alloc_object_with_shape(
        &mut self,
        _shape: &Rc<crate::value::Shape>,
        _values: Vec<VmValue>,
    ) -> VmValue {
        self.alloc_object()
    }
    fn alloc_range(&mut self, start: i64, end: i64, inclusive: bool) -> VmValue;
    fn alloc_fn(&mut self, f: NativeFn, name: &'static str) -> VmValue;
    fn alloc_class(&mut self, class: Rc<ClassObj>) -> VmValue;

    fn is_string(&self, v: VmValue) -> bool;
    fn is_array(&self, v: VmValue) -> bool;
    fn is_object(&self, _v: VmValue) -> bool {
        false
    }
    fn is_null(&self, v: VmValue) -> bool {
        v.is_null()
    }
    fn str_repr(&self, v: VmValue) -> String;
    fn str_repr_borrowed<'a>(&'a self, v: VmValue) -> Cow<'a, str> {
        Cow::Owned(self.str_repr(v))
    }
    fn str_owned(&self, v: VmValue) -> Option<String>;
    /// Zero-copy string access: a refcount bump for heap strings instead of
    /// an allocation + byte copy. Implementations back it with their shared
    /// representation; the default falls back to copying.
    fn str_shared(&self, v: VmValue) -> Option<std::rc::Rc<str>> {
        self.str_owned(v).map(std::rc::Rc::from)
    }

    fn array_len(&self, arr: VmValue) -> usize;
    fn array_get(&self, arr: VmValue, idx: usize) -> Option<VmValue>;
    fn array_set(&mut self, arr: VmValue, idx: usize, val: VmValue);
    fn array_push(&mut self, arr: VmValue, val: VmValue);
    fn array_pop(&mut self, arr: VmValue) -> Option<VmValue>;
    fn array_for_each(&self, arr: VmValue, f: &mut dyn FnMut(VmValue, usize));

    fn get_field(&self, obj: VmValue, key: &str) -> Option<VmValue>;
    fn set_field(&mut self, obj: VmValue, key: &str, val: VmValue);
    fn object_for_each(&self, _obj: VmValue, _f: &mut dyn FnMut(&str, VmValue)) {}
    fn get_object_shape(&self, _obj: VmValue) -> Option<Rc<crate::value::Shape>> {
        None
    }

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

    /// Canonicalize a value for use as a `Map`/`Set` key (see
    /// `varn_types::value::MapKey`). The default is bit-identity — correct
    /// for SSO/int/bool/null/symbol; heap-backed contexts override it to
    /// content-intern heap strings/chars/decimals/bigints and to normalize
    /// `-0.0`.
    fn map_key(&mut self, v: VmValue) -> crate::value::MapKey {
        crate::value::MapKey(v)
    }

    /// Canonical map key for a borrowed string: SSO when short-ASCII,
    /// content-interned otherwise (`intern(Value::Str)` is content-unique).
    fn str_map_key(&mut self, s: &str) -> crate::value::MapKey {
        match VmValue::try_from_sso(s) {
            Some(v) => crate::value::MapKey(v),
            None => crate::value::MapKey(self.intern(crate::Value::Str(std::rc::Rc::from(s)))),
        }
    }

    /// Record an old→young edge after storing `child` inside `parent`
    /// (collections mutated through interior mutability, which no opcode
    /// barrier sees). No-op for heap-less contexts.
    fn collection_write_barrier(&mut self, _parent: VmValue, _child: VmValue) {}

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

    /// Spawn the worker isolate and return a join task. The task resolves
    /// `Null` when the worker finishes, or rejects with a heap-independent
    /// typed error (`HostError`) if the worker threw. No port is injected —
    /// endpoints are passed in `args` and transfer by reference.
    fn spawn_isolate(
        &mut self,
        _module_path: &str,
        _export_name: &str,
        _args: Vec<crate::value::SendValue>,
    ) -> Result<crate::AsyncTask, String> {
        Err("spawn_isolate: unsupported in this context".into())
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

    fn parse_json(&mut self, _text: &str) -> Result<VmValue, String> {
        Err("parse_json not supported in this context".to_string())
    }

    fn stringify_json(&mut self, _value: VmValue) -> Result<String, String> {
        Err("stringify_json not supported in this context".to_string())
    }
}

pub type NativeFnResult = Result<VmValue, String>;
