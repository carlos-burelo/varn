use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::{RuntimeKind};
use varn_types::value::RuntimeSymbol;
use varn_types::{ClassObj, NativeCtx, Value};

pub(crate) fn typeof_val(val: VmValue, heap: &Heap) -> &'static str {
    if val.is_null() {
        return RuntimeKind::Null.name();
    }
    if val.is_bool() {
        return RuntimeKind::Bool.name();
    }
    if val.is_int() {
        return RuntimeKind::Int.name();
    }
    if val.is_f64() {
        return RuntimeKind::Float.name();
    }
    if val.is_sso() {
        return RuntimeKind::Str.name();
    }
    if val.is_heap() {
        return match heap.get(val.as_heap_idx()) {
            Some(obj) => obj.tag().name(),
            None => RuntimeKind::Object.name(),
        };
    }
    "unknown"
}

pub(crate) fn instanceof(obj: VmValue, class_nv: VmValue, heap: &Heap) -> bool {
    if !class_nv.is_heap() {
        return false;
    }
    let cls = match heap.get(class_nv.as_heap_idx()) {
        Some(HeapObj::Class(c)) => c.clone(),
        _ => return false,
    };

    match cls.name.as_str() {
        n if n == varn_core::RuntimeKind::Str.name() => {
            return obj.is_sso()
                || (obj.is_heap() && matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Str(_))))
        }
        n if n == varn_core::RuntimeKind::Int.name() => {
            return obj.is_int()
                || (obj.is_f64() && {
                    let f = obj.as_f64();
                    f == f.floor()
                })
        }
        n if n == varn_core::RuntimeKind::Float.name() => return obj.is_f64() || obj.is_int(),
        n if n == varn_core::RuntimeKind::Bool.name() => return obj.is_bool(),
        n if n == varn_core::RuntimeKind::Null.name() => return obj.is_null(),
        n if n == varn_core::RuntimeKind::Char.name() => {
            return obj.is_heap() && matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Char(_)))
        }
        n if n == varn_core::RuntimeKind::Decimal.name() => {
            return obj.is_heap()
                && matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Decimal(_)))
        }
        _ => {}
    }
    if !obj.is_heap() {
        return false;
    }
    let obj_class = match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Instance(inst)) => ClassObj::find_by_id(inst.class_id),
        Some(HeapObj::Object(o) | HeapObj::Record(o)) => o.borrow().class().clone(),
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

pub(crate) fn op_in(key: VmValue, obj: VmValue, heap: &Heap) -> bool {
    if !obj.is_heap() {
        return false;
    }
    let key_s = heap.str_repr(key);
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Object(o)) => o.borrow().get_field(&key_s).is_some(),
        Some(HeapObj::Array(a)) => {
            if let Ok(idx) = key_s.parse::<usize>() {
                return idx < a.len();
            }
            false
        }
        _ => false,
    }
}

pub(crate) fn is_array(val: VmValue, heap: &Heap) -> bool {
    val.is_heap() && matches!(heap.get(val.as_heap_idx()), Some(HeapObj::Array(_)))
}

pub(crate) fn assert_not_null(val: VmValue) -> VmResult<()> {
    if val.is_null() {
        Err(RuntimeError::new("null assertion failed"))
    } else {
        Ok(())
    }
}

pub(crate) fn get_enum_tag(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_heap() {
        if let Some(HeapObj::EnumVariant(e)) = heap.get(val.as_heap_idx()) {
            return Ok(VmValue::from_i32(e.variant_tag as i32));
        }
    }
    Err(RuntimeError::new("OpGetEnumTag: not an enum variant"))
}

pub(crate) fn get_symbol_property(
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
        // A generator is its own iterator under both protocols. `for await`
        // asks for `Symbol.asyncIterator`, and an `async function*` has to
        // answer it — but a plain `function*` answers it too, exactly as it
        // answers `Symbol.iterator`: `next()` settles its awaits before
        // returning, so the two protocols are the same object here and
        // `for await` over a sync generator is simply a no-op await per step.
        (Value::Generator(_), RuntimeSymbol::Iterator | RuntimeSymbol::AsyncIterator) => {
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

fn array_symbol_iterator(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let arr_nv = args.first().copied().unwrap_or(VmValue::null());
    let iter_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__arr", arr_nv);
    ctx.set_field(iter_nv, "__idx", VmValue::from_int(0));
    let res_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__res", res_nv);
    let extracted = ctx.extract(iter_nv);
    let next_nv = ctx.intern(Value::native_bound(extracted, array_iter_next, "next"));
    ctx.set_field(iter_nv, "next", next_nv);
    Ok(iter_nv)
}

fn array_iter_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let obj_nv = args
        .first()
        .copied()
        .ok_or("array_iter_next: missing receiver")?;
    let arr_nv = ctx.get_field(obj_nv, "__arr").unwrap_or(VmValue::null());
    let idx = ctx.as_int(ctx.get_field(obj_nv, "__idx").unwrap_or(VmValue::null()));
    let arr_len = ctx.array_len(arr_nv);
    let result_nv = ctx
        .get_field(obj_nv, "__res")
        .unwrap_or_else(|| ctx.alloc_object());
    if idx as usize >= arr_len {
        ctx.set_field(result_nv, "value", VmValue::null());
        ctx.set_field(result_nv, "done", VmValue::from_bool(true));
        return Ok(result_nv);
    }
    let item = ctx
        .array_get(arr_nv, idx as usize)
        .unwrap_or(VmValue::null());
    let idx_val = ctx.int_val(idx + 1);
    ctx.set_field(obj_nv, "__idx", idx_val);
    ctx.set_field(result_nv, "value", item);
    ctx.set_field(result_nv, "done", VmValue::from_bool(false));
    Ok(result_nv)
}

fn range_symbol_iterator(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let range_nv = args
        .first()
        .copied()
        .ok_or("range_symbol_iterator: missing receiver")?;
    if !matches!(ctx.extract(range_nv), Value::Range(_)) {
        return Err("range_symbol_iterator: invalid receiver".into());
    }
    let iter_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__range", range_nv);
    ctx.set_field(iter_nv, "__idx", VmValue::from_int(0));
    let res_nv = ctx.alloc_object();
    ctx.set_field(iter_nv, "__res", res_nv);
    let next_nv = ctx.intern(Value::native_bound(
        ctx.extract(iter_nv),
        range_iter_next,
        "next",
    ));
    ctx.set_field(iter_nv, "next", next_nv);
    Ok(iter_nv)
}

fn range_iter_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let obj_nv = args
        .first()
        .copied()
        .ok_or("range_iter_next: missing receiver")?;
    let range_nv = ctx.get_field(obj_nv, "__range").unwrap_or(VmValue::null());
    let Value::Range(range) = ctx.extract(range_nv) else {
        return Err("range_iter_next: invalid range".into());
    };
    let idx = ctx.as_int(ctx.get_field(obj_nv, "__idx").unwrap_or(VmValue::null()));
    let result_nv = ctx
        .get_field(obj_nv, "__res")
        .unwrap_or_else(|| ctx.alloc_object());
    let Some(raw) = range.nth(idx) else {
        ctx.set_field(result_nv, "value", VmValue::null());
        ctx.set_field(result_nv, "done", VmValue::from_bool(true));
        return Ok(result_nv);
    };
    let next_idx = ctx.int_val(idx + 1);
    ctx.set_field(obj_nv, "__idx", next_idx);
    let item = ctx.intern(range.element(raw));
    ctx.set_field(result_nv, "value", item);
    ctx.set_field(result_nv, "done", VmValue::from_bool(false));
    Ok(result_nv)
}

fn generator_symbol_iterator(
    _ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> varn_types::NativeFnResult {
    Ok(args.first().copied().unwrap_or(VmValue::null()))
}

pub(crate) fn bind_method(
    receiver: VmValue,
    method: VmValue,
    heap: &mut Heap,
) -> VmResult<VmValue> {
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

pub(crate) fn invoke_runtime_static(
    name: &str,
    stack: &mut Vec<VmValue>,
    heap: &mut Heap,
    flag: u16,
) -> VmResult<VmValue> {
    match name {
        varn_core::well_known::RUNTIME_RANGE => {
            let end = stack
                .pop()
                .ok_or_else(|| RuntimeError::new("range: stack empty"))?;
            let start = stack
                .pop()
                .ok_or_else(|| RuntimeError::new("range: stack empty"))?;
            let r = match (heap.extract(start), heap.extract(end)) {
                (Value::Char(a), Value::Char(b)) => varn_types::value::RangeData {
                    elem: varn_types::value::RangeElem::Char,
                    ..varn_types::value::RangeData::int(a as i64, b as i64, flag != 0)
                },
                _ => varn_types::value::RangeData::int(
                    heap.as_int(start),
                    heap.as_int(end),
                    flag != 0,
                ),
            };
            Ok(heap.intern(Value::Range(Box::new(r))))
        }
        _ => Err(RuntimeError::new(format!(
            "OpInvokeRuntimeStatic: method '{}' not supported",
            name
        ))),
    }
}
