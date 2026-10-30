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
    Error {
        class: String,
        message: String,
        fields: Vec<(String, SendValue)>,
    },
    BigInt(num_bigint::BigInt),
    Decimal(Decimal),
    Char(char),
    Array(Vec<SendValue>),
    Object(Vec<(String, SendValue)>),
    Map(Vec<(SendValue, SendValue)>),
    Set(Vec<SendValue>),
    Endpoint(super::ChannelEndpoint, u64),
    EnumVariant(Box<SendEnumVariant>),
}

#[derive(Clone, Debug)]
pub struct SendEnumVariant {
    pub enum_name: String,
    pub variant_name: String,
    pub variant_tag: i64,
    pub fields: Vec<String>,
    pub slots: Vec<SendValue>,
}

impl SendValue {
    pub fn error(class: impl Into<String>, message: impl Into<String>) -> Self {
        SendValue::Error {
            class: class.into(),
            message: message.into(),
            fields: Vec::new(),
        }
    }

    pub fn from_native_error(e: &crate::NativeError) -> Self {
        let Some(info) = e.std.as_ref() else {
            return Self::error(e.kind.class_name(), e.message.clone());
        };
        let mut fields = vec![(
            varn_core::ErrorField::Code.name().to_string(),
            SendValue::Str(info.class.code().to_string()),
        )];
        for (field, value) in &info.fields {
            let value = match value {
                crate::ErrorValue::Str(text) => SendValue::Str(text.clone()),
                crate::ErrorValue::Int(n) => SendValue::Int(*n),
            };
            fields.push((field.name().to_string(), value));
        }
        SendValue::Error {
            class: info.class.name().to_string(),
            message: e.message.clone(),
            fields,
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
            SendValue::Error {
                class,
                message,
                fields,
            } => {
                let message_nv = ctx.alloc_str(message);
                let instance = ctx.alloc_instance(class).or_else(|| {
                    ctx.alloc_instance(varn_core::RuntimeErrorKind::Error.class_name())
                });
                let Some(inst) = instance else {
                    return message_nv;
                };
                ctx.set_field(inst, varn_core::MemberKey::Message.as_str(), message_nv);
                let name_nv = ctx.alloc_str(class);
                ctx.set_field(inst, varn_core::MemberKey::Name.as_str(), name_nv);
                for (key, value) in fields {
                    let value_nv = value.to_value_ctx(ctx);
                    ctx.set_field(inst, key, value_nv);
                }
                inst
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
            SendValue::Endpoint(end, id) => end.alloc(ctx, *id).unwrap_or(VmValue::null()),
            SendValue::EnumVariant(ev) => {
                let slots = ev.slots.iter().map(|s| s.to_value_ctx(ctx)).collect();
                let def = Arc::new(crate::value::VariantDef {
                    enum_name: Arc::from(ev.enum_name.as_str()),
                    variant_name: Arc::from(ev.variant_name.as_str()),
                    tag: ev.variant_tag,
                    fields: ev.fields.iter().map(|f| Arc::from(f.as_str())).collect(),
                });
                ctx.alloc_enum_variant(crate::value::EnumVariantData {
                    enum_class_id: None,
                    def,
                    slots,
                })
            }
        }
    }
}
