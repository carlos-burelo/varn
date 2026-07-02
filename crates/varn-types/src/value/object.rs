use super::{ClassObj, RuntimeObject, RuntimeString, Value};
use crate::vm_value::{VmValue, VmValueRef};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct ObjData {
    pub inner: RuntimeObject,
}

impl Default for ObjData {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjData {
    pub fn new() -> Self {
        ObjData {
            inner: RuntimeObject::new(),
        }
    }

    pub fn from_inner(inner: RuntimeObject) -> Self {
        ObjData { inner }
    }

    pub fn new_instance(class: Rc<ClassObj>) -> Self {
        ObjData {
            inner: RuntimeObject::with_class(class),
        }
    }

    pub fn with_shape(shape: Rc<super::shape::Shape>, values: Vec<VmValue>) -> Self {
        ObjData {
            inner: RuntimeObject::with_shape(shape, values),
        }
    }

    pub fn is_instance(&self) -> bool {
        self.inner.shape.class.is_some()
    }

    pub fn class(&self) -> Option<Rc<ClassObj>> {
        self.inner.shape.class.clone()
    }

    pub fn class_name(&self) -> String {
        match &self.inner.shape.class {
            Some(c) => c.name.clone(),
            None => varn_core::TypeTag::Object.name().to_owned(),
        }
    }

    pub fn get_field(&self, key: &str) -> Option<Value> {
        self.inner.get(key).map(nv_to_value)
    }

    pub fn set_field(&mut self, key: RuntimeString, value: Value) {
        let nv = value_to_nv(&value);
        self.inner.insert(key, nv);
    }

    #[inline]
    pub fn get_field_nv(&self, key: &str) -> Option<VmValue> {
        self.inner.get(key)
    }

    #[inline]
    pub fn set_field_nv(&mut self, key: RuntimeString, value: VmValue) {
        self.inner.insert(key, value);
    }

    pub fn set_class(&mut self, class: Rc<ClassObj>) {
        self.inner.shape =
            super::shape::Shape::create(Some(class), self.inner.shape.property_names.clone());
    }
}

#[inline]
pub fn nv_to_value(nv: VmValue) -> Value {
    if nv.is_null() {
        return Value::Null;
    }
    if nv.is_bool() {
        return Value::Bool(nv.as_bool());
    }
    if nv.is_int() {
        return Value::Int(nv.as_int());
    }
    if nv.is_f64() {
        return Value::Float(nv.as_f64());
    }
    if nv.is_sso() {
        let mut buf = [0u8; 5];
        return Value::Str(Rc::from(nv.sso_as_str(&mut buf)));
    }

    Value::VmValue(Box::new(VmValueRef(nv)))
}

#[inline]
pub fn value_to_nv(v: &Value) -> VmValue {
    match v {
        Value::Null => VmValue::null(),
        Value::Bool(b) => VmValue::from_bool(*b),
        Value::Int(i) => VmValue::from_int(*i),
        Value::Float(f) => VmValue::from_f64(*f),
        Value::Str(s) => {
            if let Some(nv) = VmValue::try_from_sso(s) {
                nv
            } else {
                debug_assert!(false, "set_field: long string '{}' must be pre-interned; use set_field_nv after heap.intern_str()", s);
                VmValue::null()
            }
        }
        Value::VmValue(payload) => {
            if let Some(vr) = payload.as_any().downcast_ref::<VmValueRef>() {
                vr.0
            } else {
                debug_assert!(
                    false,
                    "set_field: Value::VmValue with non-VmValueRef payload; use set_field_nv"
                );
                VmValue::null()
            }
        }
        other => {
            debug_assert!(
                false,
                "set_field: {:?} must be pre-interned; use set_field_nv after heap.intern()",
                other
            );
            VmValue::null()
        }
    }
}
