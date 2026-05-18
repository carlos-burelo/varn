use super::{ArrayRef, ObjData, ObjRef, Value};

#[inline(always)]
pub fn new_array(v: Vec<Value>) -> Value {
    let arr_ref = ArrayRef::new(v);
    Value::Array(arr_ref)
}

#[inline(always)]
pub fn new_object(obj: ObjData) -> Value {
    let obj_ref = ObjRef::new(obj);
    Value::Object(obj_ref)
}
