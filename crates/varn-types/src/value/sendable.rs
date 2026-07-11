use crate::value::{
    alloc_array, alloc_map, alloc_object, alloc_set, new_object, nv_to_value, ObjData, Value,
};
use crate::vm_value::VmValue;
use rust_decimal::Decimal;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum SendValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(String),
    BigInt(i128),
    Decimal(Decimal),
    Char(char),
    Array(Vec<SendValue>),
    Object(std::collections::HashMap<String, SendValue>),
    Map(Vec<(SendValue, SendValue)>),
    Set(Vec<SendValue>),
    /// Endpoint de canal (id en varn_runtime::channel). Se transfiere por
    /// referencia: ambos lados comparten el mismo canal.
    ChannelSender(u64),
    ChannelReceiver(u64),
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
                {
                    let guard = obj.read();
                    if let Some(cls) = guard.class() {
                        let cname = cls.name.as_str();
                        if cname == "Sender" || cname == "Receiver" {
                            if let Some(Value::Int(id)) =
                                guard.inner.get("_chan").map(nv_to_value)
                            {
                                return Ok(if cname == "Sender" {
                                    SendValue::ChannelSender(id as u64)
                                } else {
                                    SendValue::ChannelReceiver(id as u64)
                                });
                            }
                            return Err(format!("{cname}: endpoint sin _chan"));
                        }
                    }
                }
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

/// Heap-independent marker object for a channel endpoint crossing an
/// isolate boundary. `varn-vm` (Task 3) recognizes `__chanEndpoint` on
/// materialization and mints a real `Sender`/`Receiver` instance from it.
fn endpoint_marker(dir: &str, id: u64) -> Value {
    let mut obj = ObjData::new();
    obj.set_field(
        std::rc::Rc::from("__chanEndpoint"),
        Value::Str(std::rc::Rc::from(dir)),
    );
    obj.set_field(std::rc::Rc::from("__chanId"), Value::Int(id as i64));
    new_object(obj)
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
                let g = array_ref.write();
                for item in items {
                    g.push(item.to_value());
                }
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
            SendValue::ChannelSender(id) => endpoint_marker("tx", *id),
            SendValue::ChannelReceiver(id) => endpoint_marker("rx", *id),
        }
    }

    pub fn to_value_ctx(&self, ctx: &mut dyn crate::NativeCtx) -> VmValue {
        match self {
            SendValue::Null => ctx.null_val(),
            SendValue::Bool(b) => ctx.bool_val(*b),
            SendValue::Int(n) => ctx.int_val(*n),
            SendValue::Float(bits) => ctx.intern(Value::Float(f64::from_bits(*bits))),
            SendValue::Str(s) => ctx.alloc_str(s),
            SendValue::BigInt(b) => ctx.intern(Value::BigInt(Box::new(*b))),
            SendValue::Decimal(d) => ctx.intern(Value::Decimal(Box::new(*d))),
            SendValue::Char(c) => ctx.intern(Value::Char(*c)),
            SendValue::Array(items) => {
                let mut vm_items = Vec::new();
                for item in items {
                    vm_items.push(item.to_value_ctx(ctx));
                }
                ctx.alloc_array(vm_items)
            }
            SendValue::Object(fields) => {
                let obj = ctx.alloc_object();
                for (k, v) in fields {
                    let val_nv = v.to_value_ctx(ctx);
                    ctx.set_field(obj, k, val_nv);
                }
                obj
            }
            SendValue::Map(entries) => {
                let map_ref = alloc_map();
                let mut g = map_ref.write();
                for (k, v) in entries {
                    let k_nv = k.to_value_ctx(ctx);
                    let v_nv = v.to_value_ctx(ctx);
                    g.insert(ctx.extract(k_nv), ctx.extract(v_nv));
                }
                drop(g);
                ctx.intern(Value::Map(map_ref))
            }
            SendValue::Set(items) => {
                let set_ref = alloc_set();
                let mut g = set_ref.write();
                for item in items {
                    let item_nv = item.to_value_ctx(ctx);
                    g.insert(ctx.extract(item_nv));
                }
                drop(g);
                ctx.intern(Value::Set(set_ref))
            }
            SendValue::ChannelSender(id) => endpoint_marker_ctx(ctx, "tx", *id),
            SendValue::ChannelReceiver(id) => endpoint_marker_ctx(ctx, "rx", *id),
        }
    }
}

/// `to_value_ctx` counterpart of [`endpoint_marker`]: same marker shape, but
/// built through `NativeCtx` since the destination heap is context-owned.
/// Task 3 replaces this with real instance minting once the VM hook exists.
fn endpoint_marker_ctx(ctx: &mut dyn crate::NativeCtx, dir: &str, id: u64) -> VmValue {
    let obj = ctx.alloc_object();
    let dir_val = ctx.alloc_str(dir);
    ctx.set_field(obj, "__chanEndpoint", dir_val);
    let id_val = ctx.int_val(id as i64);
    ctx.set_field(obj, "__chanId", id_val);
    obj
}

#[cfg(test)]
mod channel_endpoint_tests {
    use super::*;

    #[test]
    fn endpoint_variants_roundtrip_marker() {
        let tx = SendValue::ChannelSender(42);
        let v = tx.to_value();
        let Value::Object(o) = &v else { panic!("marker must be object") };
        let guard = o.read();
        assert!(guard.inner.contains_key("__chanEndpoint"));
        assert!(guard.inner.contains_key("__chanId"));
    }
}
