use crate::value::{
    alloc_array, alloc_map, alloc_set, new_object, nv_to_value, value_to_nv, ObjRef, Value,
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
    EnumVariant(Box<SendEnumVariant>),
}

/// Heap-independent enum variant: self-describing (name + tag + payload), so
/// the consumer mints a `Value::EnumVariant` directly — `match` on the
/// receiving side works without loading the enum's defining module.
#[derive(Clone, Debug)]
pub struct SendEnumVariant {
    pub enum_name: String,
    pub variant_name: String,
    pub variant_tag: u8,
    pub fields: Vec<String>,
    pub payload: SendValue,
}

impl SendValue {
    /// Single source of channel-endpoint detection. Given an object's class
    /// name and its `_chan` id, produce the transferable endpoint variant.
    /// `Ok(None)` for non-endpoint classes; `Err` if the class *is* an endpoint
    /// but its `_chan` field is missing. Shared by [`Value::to_sendable`] and
    /// the VM's `ExecCtx` heap-walking converters so endpoint detection lives
    /// in exactly one place.
    pub fn endpoint_for(
        class_name: &str,
        chan_id: Option<i64>,
    ) -> Result<Option<SendValue>, String> {
        match class_name {
            "Sender" | "Receiver" => match chan_id {
                Some(id) if class_name == "Sender" => Ok(Some(SendValue::ChannelSender(id as u64))),
                Some(id) => Ok(Some(SendValue::ChannelReceiver(id as u64))),
                None => Err(format!("{class_name}: endpoint sin _chan")),
            },
            _ => Ok(None),
        }
    }
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
                        let chan_id = match guard.get("_chan").map(nv_to_value) {
                            Some(Value::Int(id)) => Some(id),
                            _ => None,
                        };
                        if let Some(sv) = SendValue::endpoint_for(cls.name.as_str(), chan_id)? {
                            return Ok(sv);
                        }
                    }
                }
                let mut map = std::collections::HashMap::new();
                for (k, nv) in obj.read().iter() {
                    let v = nv_to_value(nv);
                    map.insert(k.to_string(), v.to_sendable()?);
                }
                Ok(SendValue::Object(map))
            }
            // Map/Set entries are raw VmValues; without a heap only
            // scalar/SSO entries convert (`nv_to_value` wraps heap refs as
            // opaque payloads, which the catch-all below rejects). The
            // heap-aware channel path (`NativeCtx::to_sendable` in the VM)
            // handles arbitrary entries and is what `send` actually uses.
            Value::Map(map_ref) => {
                let mut items = Vec::new();
                for (k, v) in map_ref.read().iter() {
                    items.push((
                        nv_to_value(k.0).to_sendable()?,
                        nv_to_value(*v).to_sendable()?,
                    ));
                }
                Ok(SendValue::Map(items))
            }
            Value::Set(set_ref) => {
                let mut items = Vec::new();
                for v in set_ref.read().iter() {
                    items.push(nv_to_value(v.0).to_sendable()?);
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
            Value::EnumVariant(d) => Ok(SendValue::EnumVariant(Box::new(SendEnumVariant {
                enum_name: d.enum_name.to_string(),
                variant_name: d.variant_name.to_string(),
                variant_tag: d.variant_tag,
                fields: d.fields.iter().map(|f| f.to_string()).collect(),
                payload: d.payload.to_sendable()?,
            }))),
            _ => Err(format!("Value cannot be sent to an isolate")),
        }
    }
}

/// Heap-independent marker object for a channel endpoint crossing an
/// isolate boundary. `varn-vm` (Task 3) recognizes `__chanEndpoint` on
/// materialization and mints a real `Sender`/`Receiver` instance from it.
fn endpoint_marker(dir: &str, id: u64) -> Value {
    let obj = ObjRef::from_pairs([
        (
            std::rc::Rc::from("__chanEndpoint"),
            value_to_nv(&Value::Str(std::rc::Rc::from(dir))),
        ),
        (
            std::rc::Rc::from("__chanId"),
            value_to_nv(&Value::Int(id as i64)),
        ),
    ]);
    new_object(obj)
}

impl SendValue {
    /// True for values that `ObjData::set_field` / [`value_to_nv`] can embed
    /// heap-independently, so the channel's parked-receiver handoff can keep
    /// delivering them inside a plain `{value, done}` object. Everything else
    /// (strings, composites, endpoints, decimals, …) is carried by
    /// [`SendEnvelope`] and materialized on the consumer's heap. Kept
    /// deliberately narrow — matches exactly the arms `value_to_nv` handles
    /// without a fallible SSO / interning step.
    pub fn is_direct_scalar(&self) -> bool {
        matches!(
            self,
            SendValue::Null | SendValue::Bool(_) | SendValue::Int(_) | SendValue::Float(_)
        )
    }

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
            SendValue::Object(fields) => Value::Object(ObjRef::from_pairs(
                fields
                    .iter()
                    .map(|(k, v)| (Rc::from(k.as_str()), value_to_nv(&v.to_value()))),
            )),
            // Heap-free materialization only round-trips scalar/SSO entries
            // (all `value_to_nv` handles); the ctx variant below covers the
            // rest and is what channel delivery uses.
            SendValue::Map(entries) => {
                let map_ref = alloc_map();
                let mut g = map_ref.write();
                for (k, v) in entries {
                    g.insert(
                        crate::value::MapKey(value_to_nv(&k.to_value())),
                        value_to_nv(&v.to_value()),
                    );
                }
                drop(g);
                Value::Map(map_ref)
            }
            SendValue::Set(items) => {
                let set_ref = alloc_set();
                let mut g = set_ref.write();
                for item in items {
                    g.insert(crate::value::MapKey(value_to_nv(&item.to_value())));
                }
                drop(g);
                Value::Set(set_ref)
            }
            SendValue::ChannelSender(id) => endpoint_marker("tx", *id),
            SendValue::ChannelReceiver(id) => endpoint_marker("rx", *id),
            SendValue::EnumVariant(ev) => {
                Value::EnumVariant(Box::new(crate::value::EnumVariantData {
                    enum_name: Rc::from(ev.enum_name.as_str()),
                    variant_name: Rc::from(ev.variant_name.as_str()),
                    variant_tag: ev.variant_tag,
                    fields: ev.fields.iter().map(|f| Rc::from(f.as_str())).collect(),
                    payload: ev.payload.to_value(),
                }))
            }
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
                    let key = ctx.map_key(k_nv);
                    g.insert(key, v_nv);
                }
                drop(g);
                ctx.intern(Value::Map(map_ref))
            }
            SendValue::Set(items) => {
                let set_ref = alloc_set();
                let mut g = set_ref.write();
                for item in items {
                    let item_nv = item.to_value_ctx(ctx);
                    let key = ctx.map_key(item_nv);
                    g.insert(key);
                }
                drop(g);
                ctx.intern(Value::Set(set_ref))
            }
            SendValue::ChannelSender(id) => endpoint_marker_ctx(ctx, "tx", *id),
            SendValue::ChannelReceiver(id) => endpoint_marker_ctx(ctx, "rx", *id),
            SendValue::EnumVariant(ev) => {
                let payload_nv = ev.payload.to_value_ctx(ctx);
                ctx.intern(Value::EnumVariant(Box::new(
                    crate::value::EnumVariantData {
                        enum_name: Rc::from(ev.enum_name.as_str()),
                        variant_name: Rc::from(ev.variant_name.as_str()),
                        variant_tag: ev.variant_tag,
                        fields: ev.fields.iter().map(|f| Rc::from(f.as_str())).collect(),
                        payload: ctx.extract(payload_nv),
                    },
                )))
            }
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

/// Heap-independent carrier for a non-scalar payload delivered by a channel's
/// direct/parked receiver handoff. The producing thread never touches the
/// consumer's GC heap; the consumer's await-resume hook (`varn-vm`
/// `host_values::open_resolved`) materializes `sv` via
/// [`SendValue::to_value_ctx`] on its own heap.
///
/// `wrap` distinguishes the two consumer shapes that share one channel task:
/// `Receiver::next` needs a `{value, done:false}` object (for-await protocol),
/// while `Receiver::receive` wants the bare value. The channel produces
/// `wrap:false`; `next` re-wraps to `wrap:true`.
#[derive(Debug, Clone)]
pub struct SendEnvelope {
    pub sv: SendValue,
    pub wrap: bool,
}

impl varn_core::VmValuePayload for SendEnvelope {
    fn clone_payload(&self) -> Box<dyn varn_core::VmValuePayload> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl SendEnvelope {
    /// Wrap a non-scalar payload for delivery (consumer materializes later).
    pub fn deliver(sv: SendValue) -> Value {
        Value::VmValue(Box::new(SendEnvelope { sv, wrap: false }))
    }

    /// Borrow the envelope out of a resolved `Value`, if it is one.
    pub fn from_value(v: &Value) -> Option<&SendEnvelope> {
        if let Value::VmValue(payload) = v {
            payload.as_any().downcast_ref::<SendEnvelope>()
        } else {
            None
        }
    }
}

/// Heap-independent typed host error. Builtins reject tasks with this payload
/// when no consumer ctx is available (e.g. inside `on_settle` callbacks); the
/// VM's await-resume hook (`host_values::open_rejected`) mints a real instance
/// of the named intrinsic class on the consumer's heap so `instanceof` works.
///
/// Note: this deliberately replaces the `{__hostErrorClass, message}` marker
/// *object* — a bare `ObjData` cannot embed non-SSO strings (`value_to_nv`
/// nulls them in release builds), so class names >5 bytes would be lost.
#[derive(Debug, Clone)]
pub struct HostError {
    pub class: String,
    pub message: String,
}

impl varn_core::VmValuePayload for HostError {
    fn clone_payload(&self) -> Box<dyn varn_core::VmValuePayload> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl HostError {
    pub fn to_value(class: &str, message: &str) -> Value {
        Value::VmValue(Box::new(HostError {
            class: class.to_string(),
            message: message.to_string(),
        }))
    }

    pub fn from_value(v: &Value) -> Option<&HostError> {
        if let Value::VmValue(payload) = v {
            payload.as_any().downcast_ref::<HostError>()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod channel_endpoint_tests {
    use super::*;

    #[test]
    fn host_error_roundtrips() {
        let v = HostError::to_value("ChannelClosed", "channel closed");
        let he = HostError::from_value(&v).expect("must downcast");
        assert_eq!(he.class, "ChannelClosed");
        assert_eq!(he.message, "channel closed");
    }

    #[test]
    fn send_envelope_roundtrips_payload() {
        let env =
            SendEnvelope::deliver(SendValue::Array(vec![SendValue::Int(1), SendValue::Int(2)]));
        let got = SendEnvelope::from_value(&env).expect("must be envelope");
        assert!(!got.wrap);
        assert!(matches!(&got.sv, SendValue::Array(items) if items.len() == 2));
    }

    #[test]
    fn enum_variant_roundtrips_through_sendable() {
        let original = Value::EnumVariant(Box::new(crate::value::EnumVariantData {
            enum_name: Rc::from("Msg"),
            variant_name: Rc::from("Val"),
            variant_tag: 0,
            fields: vec![],
            payload: Value::Int(7),
        }));
        let sv = original.to_sendable().expect("enum must be sendable");
        let SendValue::EnumVariant(ev) = &sv else {
            panic!("must convert to SendValue::EnumVariant")
        };
        assert_eq!(ev.enum_name, "Msg");
        assert_eq!(ev.variant_name, "Val");
        assert_eq!(ev.variant_tag, 0);
        let back = sv.to_value();
        let Value::EnumVariant(d) = &back else {
            panic!("must materialize as Value::EnumVariant")
        };
        assert_eq!(d.enum_name.as_ref(), "Msg");
        assert!(matches!(d.payload, Value::Int(7)));
    }

    #[test]
    fn endpoint_variants_roundtrip_marker() {
        let tx = SendValue::ChannelSender(42);
        let v = tx.to_value();
        let Value::Object(o) = &v else {
            panic!("marker must be object")
        };
        let guard = o.read();
        assert!(guard.contains_key("__chanEndpoint"));
        assert!(guard.contains_key("__chanId"));
    }
}
