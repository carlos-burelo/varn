use std::rc::Rc;
use crate::value::{Value, alloc_array, alloc_object, alloc_map, alloc_set, nv_to_value};
use rust_decimal::Decimal;

#[derive(Clone, Debug)]
pub enum SendValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64), // Representación de bits de f64
    Str(String),
    BigInt(i128),
    Decimal(Decimal),
    Char(char),
    Array(Vec<SendValue>),
    Object(std::collections::HashMap<String, SendValue>),
    Map(Vec<(SendValue, SendValue)>),
    Set(Vec<SendValue>),
}

impl Value {
    pub fn to_sendable(&self) -> Result<SendValue, String> {
        match self {
            Value::Null => Ok(SendValue::Null),
            Value::Bool(b) => Ok(SendValue::Bool(*b)),
            Value::Int(n) => Ok(SendValue::Int(*n)),
            Value::Float(f) => Ok(SendValue::Float(f.to_bits())),
            Value::Str(s) => Ok(SendValue::Str(s.to_string())),
            Value::BigInt(b) => Ok(SendValue::BigInt(**b)),
            Value::Decimal(d) => Ok(SendValue::Decimal(**d)),
            Value::Char(c) => Ok(SendValue::Char(*c)),
            Value::Array(arr) => {
                let mut items = Vec::new();
                for item in arr.read().iter() {
                    items.push(item.to_sendable()?);
                }
                Ok(SendValue::Array(items))
            }
            Value::Object(obj) => {
                let mut map = std::collections::HashMap::new();
                for (k, nv) in obj.read().inner.iter() {
                    let v = nv_to_value(nv);
                    map.insert(k.to_string(), v.to_sendable()?);
                }
                Ok(SendValue::Object(map))
            }
            Value::Map(map_ref) => {
                let mut items = Vec::new();
                for (k, v) in map_ref.read().iter() {
                    items.push((k.to_sendable()?, v.to_sendable()?));
                }
                Ok(SendValue::Map(items))
            }
            Value::Set(set_ref) => {
                let mut items = Vec::new();
                for v in set_ref.read().iter() {
                    items.push(v.to_sendable()?);
                }
                Ok(SendValue::Set(items))
            }
            Value::Range(r) => {
                let mut fields = std::collections::HashMap::new();
                fields.insert("start".to_string(), SendValue::Int(r.start));
                fields.insert("end".to_string(), SendValue::Int(r.end));
                fields.insert("inclusive".to_string(), SendValue::Bool(r.inclusive));
                fields.insert("step".to_string(), SendValue::Int(r.step));
                Ok(SendValue::Object(fields))
            }
            _ => Err(format!("Value cannot be sent to an isolate")),
        }
    }
}

impl SendValue {
    pub fn to_value(&self) -> Value {
        match self {
            SendValue::Null => Value::Null,
            SendValue::Bool(b) => Value::Bool(*b),
            SendValue::Int(n) => Value::Int(*n),
            SendValue::Float(bits) => Value::Float(f64::from_bits(*bits)),
            SendValue::Str(s) => Value::Str(Rc::from(s.as_str())),
            SendValue::BigInt(b) => Value::BigInt(Box::new(*b)),
            SendValue::Decimal(d) => Value::Decimal(Box::new(*d)),
            SendValue::Char(c) => Value::Char(*c),
            SendValue::Array(items) => {
                let array_ref = alloc_array();
                let mut g = array_ref.write();
                for item in items {
                    g.push(item.to_value());
                }
                drop(g);
                Value::Array(array_ref)
            }
            SendValue::Object(fields) => {
                let obj_ref = alloc_object();
                let mut g = obj_ref.write();
                for (k, v) in fields {
                    g.set_field(Rc::from(k.as_str()), v.to_value());
                }
                drop(g);
                Value::Object(obj_ref)
            }
            SendValue::Map(entries) => {
                let map_ref = alloc_map();
                let mut g = map_ref.write();
                for (k, v) in entries {
                    g.insert(k.to_value(), v.to_value());
                }
                drop(g);
                Value::Map(map_ref)
            }
            SendValue::Set(items) => {
                let set_ref = alloc_set();
                let mut g = set_ref.write();
                for item in items {
                    g.insert(item.to_value());
                }
                drop(g);
                Value::Set(set_ref)
            }
        }
    }
}
