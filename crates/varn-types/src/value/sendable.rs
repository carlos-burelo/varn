use crate::vm_value::VmValue;
use bigdecimal::BigDecimal as Decimal;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum SendValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(String),
    Bytes(Vec<u8>),
    Error { class: String, message: String },
    BigInt(num_bigint::BigInt),
    Decimal(Decimal),
    Char(char),
    Array(Vec<SendValue>),
    Object(rustc_hash::FxHashMap<String, SendValue>),
    Map(Vec<(SendValue, SendValue)>),
    Set(Vec<SendValue>),
    ChannelSender(u64),
    ChannelReceiver(u64),
    EnumVariant(Box<SendEnumVariant>),
}

#[derive(Clone, Debug)]
pub struct SendEnumVariant {
    pub enum_name: String,
    pub variant_name: String,
    pub variant_tag: i64,
    pub fields: Vec<String>,
    pub payload: SendValue,
}

impl SendValue {
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

    pub fn to_value_ctx(&self, ctx: &mut dyn crate::NativeCtx) -> VmValue {
        match self {
            SendValue::Null => ctx.null_val(),
            SendValue::Bool(b) => ctx.bool_val(*b),
            SendValue::Int(n) => ctx.int_val(*n),
            SendValue::Float(bits) => VmValue::from_f64(f64::from_bits(*bits)),
            SendValue::Str(s) => ctx.alloc_str(s),
            SendValue::Bytes(b) => ctx.alloc_buffer_from_bytes(b),
            SendValue::Error { class, message } => {
                let message_nv = ctx.alloc_str(message);
                match ctx.alloc_instance(class) {
                    Some(inst) => {
                        ctx.set_field(inst, "message", message_nv);
                        let name_nv = ctx.alloc_str(class);
                        ctx.set_field(inst, "name", name_nv);
                        inst
                    }
                    None => message_nv,
                }
            }
            SendValue::BigInt(b) => ctx.alloc_bigint(b.clone()),
            SendValue::Decimal(d) => ctx.alloc_decimal(d.clone()),
            SendValue::Char(c) => ctx.alloc_char(*c),
            SendValue::Array(items) => {
                let mut vm_items = Vec::with_capacity(items.len());
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
                let mut pairs = Vec::with_capacity(entries.len());
                for (k, v) in entries {
                    let k_nv = k.to_value_ctx(ctx);
                    let v_nv = v.to_value_ctx(ctx);
                    pairs.push((k_nv, v_nv));
                }
                ctx.alloc_map(pairs)
            }
            SendValue::Set(items) => {
                let mut vm_items = Vec::with_capacity(items.len());
                for item in items {
                    vm_items.push(item.to_value_ctx(ctx));
                }
                ctx.alloc_set(vm_items)
            }
            SendValue::ChannelSender(id) => endpoint_marker(ctx, "tx", *id),
            SendValue::ChannelReceiver(id) => endpoint_marker(ctx, "rx", *id),
            SendValue::EnumVariant(ev) => {
                let payload = ev.payload.to_value_ctx(ctx);
                ctx.alloc_enum_variant(crate::value::EnumVariantData {
                    enum_class_id: None,
                    enum_name: Arc::from(ev.enum_name.as_str()),
                    variant_name: Arc::from(ev.variant_name.as_str()),
                    variant_tag: ev.variant_tag,
                    fields: ev.fields.iter().map(|f| Arc::from(f.as_str())).collect(),
                    payload,
                })
            }
        }
    }
}

fn endpoint_marker(ctx: &mut dyn crate::NativeCtx, dir: &str, id: u64) -> VmValue {
    let obj = ctx.alloc_object();
    let dir_val = ctx.alloc_str(dir);
    ctx.set_field(obj, "__chanEndpoint", dir_val);
    let id_val = ctx.int_val(id as i64);
    ctx.set_field(obj, "__chanId", id_val);
    obj
}
