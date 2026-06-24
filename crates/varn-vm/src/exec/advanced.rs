use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_core::{IntrinsicType, RuntimeTypeName};
use varn_types::value::{EnumVariantData, RuntimeSymbol};
use varn_types::{NativeCtx, Value};

pub fn typeof_val(val: VmValue, heap: &Heap) -> &'static str {
    if val.is_null() {
        return IntrinsicType::Null.as_str();
    }
    if val.is_bool() {
        return IntrinsicType::Bool.as_str();
    }
    if val.is_int() {
        return IntrinsicType::Int.as_str();
    }
    if val.is_f64() {
        return IntrinsicType::Float.as_str();
    }
    if val.is_sso() {
        return IntrinsicType::Str.as_str();
    }
    if val.is_heap() {
        return match heap.get(val.as_heap_idx()) {
            Some(HeapObj::Str(_)) => IntrinsicType::Str.as_str(),
            Some(HeapObj::VmClosure(_))
            | Some(HeapObj::NativeFn(..))
            | Some(HeapObj::BoundMethod(..)) => "function",
            Some(HeapObj::Class(_)) => RuntimeTypeName::Class.as_str(),
            Some(HeapObj::Array(_)) => RuntimeTypeName::Array.as_str(),
            Some(HeapObj::Object(_))
            | Some(HeapObj::Module(_))
            | Some(HeapObj::FrozenModule(_)) => RuntimeTypeName::Object.as_str(),
            Some(HeapObj::Map(_)) => "map",
            Some(HeapObj::Set(_)) => "set",
            Some(HeapObj::BigInt(_)) => IntrinsicType::BigInt.as_str(),
            Some(HeapObj::Decimal(_)) => IntrinsicType::Decimal.as_str(),
            Some(HeapObj::Char(_)) => IntrinsicType::Char.as_str(),
            Some(HeapObj::Symbol(_)) => IntrinsicType::Symbol.as_str(),
            Some(HeapObj::EnumVariant(_)) => RuntimeTypeName::Enum.as_str(),
            Some(HeapObj::Range(_)) => RuntimeTypeName::Range.as_str(),
            _ => RuntimeTypeName::Object.as_str(),
        };
    }
    "unknown"
}

pub fn instanceof(obj: VmValue, class_nv: VmValue, heap: &Heap) -> bool {
    if !class_nv.is_heap() {
        return false;
    }
    let cls = match heap.get(class_nv.as_heap_idx()) {
        Some(HeapObj::Class(c)) => c.clone(),
        _ => return false,
    };

    match cls.name.as_str() {
        n if n == IntrinsicType::Str.as_str() => {
            return obj.is_sso()
                || (obj.is_heap() && matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Str(_))))
        }
        n if n == IntrinsicType::Int.as_str() => {
            return obj.is_int()
                || (obj.is_f64() && {
                    let f = obj.as_f64();
                    f == f.floor()
                })
        }
        n if n == IntrinsicType::Float.as_str() => return obj.is_f64() || obj.is_int(),
        n if n == IntrinsicType::Bool.as_str() => return obj.is_bool(),
        n if n == IntrinsicType::Null.as_str() => return obj.is_null(),
        n if n == IntrinsicType::Char.as_str() => {
            return obj.is_heap() && matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Char(_)))
        }
        n if n == IntrinsicType::Decimal.as_str() => {
            return obj.is_heap()
                && matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Decimal(_)))
        }
        _ => {}
    }
    if !obj.is_heap() {
        return false;
    }
    let obj_class = match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Object(o)) => o.borrow().class().clone(),
        _ => return false,
    };
    let mut cur = obj_class;
    while let Some(c) = cur {
        if c.id == cls.id {
            return true;
        }
        cur = c.superclass.borrow().clone();
    }
    false
}

pub fn op_in(key: VmValue, obj: VmValue, heap: &Heap) -> bool {
    if !obj.is_heap() {
        return false;
    }
    let key_s = heap.str_repr(key);
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Object(o)) => o.borrow().get_field(&key_s).is_some(),
        Some(HeapObj::Array(a)) => {
            if let Ok(idx) = key_s.parse::<usize>() {
                return idx < a.borrow().len();
            }
            false
        }
        _ => false,
    }
}

pub fn is_array(val: VmValue, heap: &Heap) -> bool {
    val.is_heap() && matches!(heap.get(val.as_heap_idx()), Some(HeapObj::Array(_)))
}

pub fn is_null(val: VmValue) -> bool {
    val.is_null()
}

pub fn assert_not_null(val: VmValue) -> VmResult<()> {
    if val.is_null() {
        Err(RuntimeError::new("null assertion failed"))
    } else {
        Ok(())
    }
}

pub fn make_enum_variant(tag: u8, name: &str, payload: VmValue, heap: &mut Heap) -> VmValue {
    let payload_val = heap.extract(payload);
    let (name_part, fields_part) = match name.find(':') {
        Some(idx) => (&name[..idx], &name[idx + 1..]),
        None => (name, ""),
    };
    let (enum_name_str, variant_name_str) = match name_part.rfind('.') {
        Some(idx) => (&name_part[..idx], &name_part[idx + 1..]),
        None => ("", name_part),
    };
    let fields: Vec<Rc<str>> = if fields_part.is_empty() {
        vec![]
    } else {
        fields_part.split(',').map(Rc::from).collect()
    };

    let data = Box::new(EnumVariantData {
        enum_name: Rc::from(enum_name_str),
        variant_name: Rc::from(variant_name_str),
        variant_tag: tag,
        fields,
        payload: payload_val,
    });
    VmValue::from_heap_idx(heap.alloc(HeapObj::EnumVariant(data)))
}

pub fn get_enum_tag(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_heap() {
        if let Some(HeapObj::EnumVariant(e)) = heap.get(val.as_heap_idx()) {
            return Ok(VmValue::from_i32(e.variant_tag as i32));
        }
    }
    Err(RuntimeError::new("OpGetEnumTag: not an enum variant"))
}

pub fn wrap_spread(val: VmValue) -> VmValue {
    val
}

pub fn get_symbol_property(
    obj: VmValue,
    symbol: RuntimeSymbol,
    heap: &mut Heap,
) -> VmResult<VmValue> {
    let val = heap.extract(obj);
    let sym_str = symbol.to_string();
    let result = match (&val, symbol) {
        (Value::Array(_), RuntimeSymbol::Iterator) => {
            Value::native_bound(val.clone(), array_symbol_iterator, "[Symbol.iterator]")
        }
        (Value::Range(_), RuntimeSymbol::Iterator) => {
            Value::native_bound(val.clone(), range_symbol_iterator, "[Symbol.iterator]")
        }
        (Value::Generator(_), RuntimeSymbol::Iterator) => {
            Value::native_bound(val.clone(), generator_symbol_iterator, "[Symbol.iterator]")
        }
        (Value::Object(o), _) => {
            let guard = o.borrow();
            guard.get_field(sym_str.as_str()).unwrap_or(Value::Null)
        }
        _ => Value::Null,
    };
    Ok(heap.intern(result))
}

fn array_symbol_iterator(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let arr_nv = args.first().copied().unwrap_or(VmValue::null());
    let iter_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__arr", arr_nv);
    ctx.set_field(iter_nv, "__idx", VmValue::from_int(0));
    let extracted = ctx.extract(iter_nv);
    let next_nv = ctx.intern(Value::native_bound(extracted, array_iter_next, "next"));
    ctx.set_field(iter_nv, "next", next_nv);
    Ok(iter_nv)
}

fn array_iter_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let obj_nv = args
        .first()
        .copied()
        .ok_or("array_iter_next: missing receiver")?;
    let arr_nv = ctx.get_field(obj_nv, "__arr").unwrap_or(VmValue::null());
    let idx = ctx
        .get_field(obj_nv, "__idx")
        .unwrap_or(VmValue::null())
        .to_i32();
    let arr_len = ctx.array_len(arr_nv);
    let result_nv = ctx.alloc_object();
    if idx as usize >= arr_len {
        ctx.set_field(result_nv, "value", VmValue::null());
        ctx.set_field(result_nv, "done", VmValue::from_bool(true));
        return Ok(result_nv);
    }
    let item = ctx
        .array_get(arr_nv, idx as usize)
        .unwrap_or(VmValue::null());
    ctx.set_field(obj_nv, "__idx", VmValue::from_int((idx + 1) as i64));
    ctx.set_field(result_nv, "value", item);
    ctx.set_field(result_nv, "done", VmValue::from_bool(false));
    Ok(result_nv)
}

fn range_symbol_iterator(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let range_nv = args
        .first()
        .copied()
        .ok_or("range_symbol_iterator: missing receiver")?;
    let range_val = ctx.extract(range_nv);
    let (cur, end_excl, step) = match range_val {
        Value::Range(r) => (r.start, if r.inclusive { r.end + 1 } else { r.end }, r.step),
        _ => return Err("range_symbol_iterator: invalid receiver".into()),
    };
    let iter_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__cur", VmValue::from_int(cur));
    ctx.set_field(iter_nv, "__end", VmValue::from_int(end_excl));
    ctx.set_field(iter_nv, "__step", VmValue::from_int(step));
    let next_nv = ctx.intern(Value::native_bound(
        ctx.extract(iter_nv),
        range_iter_next,
        "next",
    ));
    ctx.set_field(iter_nv, "next", next_nv);
    Ok(iter_nv)
}

fn range_iter_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let obj_nv = args
        .first()
        .copied()
        .ok_or("range_iter_next: missing receiver")?;
    let cur = ctx
        .get_field(obj_nv, "__cur")
        .unwrap_or(VmValue::null())
        .to_i32() as i64;
    let end = ctx
        .get_field(obj_nv, "__end")
        .unwrap_or(VmValue::null())
        .to_i32() as i64;
    let step = ctx
        .get_field(obj_nv, "__step")
        .map(|v| v.to_i32() as i64)
        .unwrap_or(1);
    let result_nv = ctx.alloc_object();
    if cur >= end {
        ctx.set_field(result_nv, "value", VmValue::null());
        ctx.set_field(result_nv, "done", VmValue::from_bool(true));
        return Ok(result_nv);
    }
    ctx.set_field(obj_nv, "__cur", VmValue::from_int(cur + step));
    ctx.set_field(result_nv, "value", VmValue::from_int(cur));
    ctx.set_field(result_nv, "done", VmValue::from_bool(false));
    Ok(result_nv)
}

fn generator_symbol_iterator(
    _ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> Result<VmValue, String> {
    Ok(args.first().copied().unwrap_or(VmValue::null()))
}

pub fn bind_method(receiver: VmValue, method: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    let recv_val = heap.extract(receiver);
    let method_val = heap.extract(method);
    match method_val {
        Value::NativeFn(b) => Ok(heap.intern(Value::native_bound(recv_val, b.0, b.1))),
        Value::VmValue(payload) if !recv_val.is_null() => {
            Ok(heap.intern(Value::vm_bound(recv_val, payload, None)))
        }
        other => Ok(heap.intern(other)),
    }
}

pub fn get_symbol(kind: varn_types::value::RuntimeSymbol, heap: &mut Heap) -> VmValue {
    heap.intern(Value::Symbol(kind))
}

pub fn invoke_runtime_static(
    name: &str,
    stack: &mut Vec<VmValue>,
    heap: &mut Heap,
    flag: u16,
) -> VmResult<VmValue> {
    match name {
        "__range__" => {
            let end = stack
                .pop()
                .ok_or_else(|| RuntimeError::new("range: stack empty"))?;
            let start = stack
                .pop()
                .ok_or_else(|| RuntimeError::new("range: stack empty"))?;
            let r = varn_types::value::RangeData {
                start: start.to_i32() as i64,
                end: end.to_i32() as i64,
                inclusive: flag != 0,
                step: 1,
            };
            Ok(heap.intern(Value::Range(Box::new(r))))
        }
        _ => Err(RuntimeError::new(format!(
            "OpInvokeRuntimeStatic: method '{}' not supported",
            name
        ))),
    }
}
