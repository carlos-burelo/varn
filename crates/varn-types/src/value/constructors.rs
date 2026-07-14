use super::{ArrayRef, ObjRef, Value};

#[inline(always)]
pub fn new_array(v: Vec<Value>) -> Value {
    let arr_ref = ArrayRef::new(v);
    Value::Array(arr_ref)
}

#[inline(always)]
pub fn new_object(obj: ObjRef) -> Value {
    Value::Object(obj)
}
