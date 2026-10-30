use crate::exec::ctx::ExecCtx;
use crate::heap::HeapObj;
use crate::value::VmValue;
use varn_types::NativeCtx;

pub(super) fn to_sendable(
    ctx: &ExecCtx,
    val: VmValue,
) -> Result<varn_types::value::SendValue, String> {
    if val.is_null() {
        return Ok(varn_types::value::SendValue::Null);
    }
    if val.is_bool() {
        return Ok(varn_types::value::SendValue::Bool(val.as_bool()));
    }
    if val.is_int() {
        return Ok(varn_types::value::SendValue::Int(val.as_int()));
    }
    if val.is_f64() {
        return Ok(varn_types::value::SendValue::Float(val.as_f64().to_bits()));
    }
    if val.is_sso() {
        let mut buf = [0u8; varn_types::vm_value::SSO_MAX_LEN];
        return Ok(varn_types::value::SendValue::Str(
            val.sso_as_str(&mut buf).to_owned(),
        ));
    }
    if val.is_heap() {
        if let Some(inst) = ctx.heap.instance_of(val)
            && varn_types::ClassObj::find_by_id(inst.class_id)
                .is_some_and(|cls| crate::task::derives_from_error(&cls))
            && let Some(error) = crate::task::error_value(&ctx.heap, val)
        {
            return Ok(error);
        }
        match ctx.heap.get(val.as_heap()) {
            Some(HeapObj::Str(s)) => Ok(varn_types::value::SendValue::Str(s.to_string())),
            Some(HeapObj::Array(arr)) => {
                let mut items = Vec::with_capacity(arr.len());
                for i in 0..arr.len() {
                    items.push(ctx.to_sendable(arr.get_vm(i).unwrap())?);
                }
                Ok(varn_types::value::SendValue::Array(items))
            }
            Some(HeapObj::Object(obj)) => {
                let borrow = obj.borrow();

                if borrow
                    .class()
                    .is_some_and(|cls| crate::task::derives_from_error(&cls))
                    && let Some(error) = crate::task::error_value(&ctx.heap, val)
                {
                    return Ok(error);
                }

                if let Some(end) = borrow
                    .class()
                    .and_then(|cls| varn_types::value::ChannelEndpoint::of_class(&cls))
                {
                    let id = borrow
                        .get(varn_types::value::ENDPOINT_ID_FIELD)
                        .filter(|v| v.is_int())
                        .map(|v| v.as_int() as u64)
                        .ok_or_else(|| {
                            format!("{}: endpoint without a channel id", end.class_name())
                        })?;
                    return Ok(varn_types::value::SendValue::Endpoint(end, id));
                }
                let mut map = Vec::new();
                for (k, nv) in borrow.iter() {
                    map.push((k.to_string(), ctx.to_sendable(nv)?));
                }
                Ok(varn_types::value::SendValue::Object(map))
            }
            Some(HeapObj::Map(map_ref)) => {
                let map_ref = *map_ref;
                let mut items = Vec::new();
                for (k, v) in map_ref.read().iter() {
                    items.push((ctx.to_sendable(k.0)?, ctx.to_sendable(*v)?));
                }
                Ok(varn_types::value::SendValue::Map(items))
            }
            Some(HeapObj::Set(set_ref)) => {
                let set_ref = *set_ref;
                let mut items = Vec::new();
                for v in set_ref.read().iter() {
                    items.push(ctx.to_sendable(v.0)?);
                }
                Ok(varn_types::value::SendValue::Set(items))
            }
            Some(HeapObj::BigInt(b)) => Ok(varn_types::value::SendValue::BigInt((**b).clone())),
            Some(HeapObj::Decimal(d)) => Ok(varn_types::value::SendValue::Decimal((**d).clone())),
            Some(HeapObj::Char(c)) => Ok(varn_types::value::SendValue::Char(*c)),
            Some(HeapObj::EnumVariant(d)) => {
                let d = d.clone();
                let slots = d
                    .slots
                    .iter()
                    .map(|s| ctx.to_sendable(*s))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(varn_types::value::SendValue::EnumVariant(Box::new(
                    varn_types::value::SendEnumVariant {
                        enum_name: d.def.enum_name.to_string(),
                        variant_name: d.def.variant_name.to_string(),
                        variant_tag: d.tag(),
                        fields: d.def.fields.iter().map(|f| f.to_string()).collect(),
                        slots,
                    },
                )))
            }
            Some(HeapObj::Range(r)) => {
                let fields = vec![
                    (
                        varn_core::MemberKey::Start.as_str().to_string(),
                        varn_types::value::SendValue::Int(r.start),
                    ),
                    (
                        varn_core::MemberKey::End.as_str().to_string(),
                        varn_types::value::SendValue::Int(r.end),
                    ),
                    (
                        varn_core::MemberKey::Inclusive.as_str().to_string(),
                        varn_types::value::SendValue::Bool(r.inclusive),
                    ),
                    (
                        "step".to_string(),
                        varn_types::value::SendValue::Int(r.step),
                    ),
                ];
                Ok(varn_types::value::SendValue::Object(fields))
            }
            Some(
                HeapObj::Tuple(_)
                | HeapObj::Record(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => Err("Value cannot be sent to an isolate".to_string()),
        }
    } else {
        Err("Value cannot be sent to an isolate".to_string())
    }
}
