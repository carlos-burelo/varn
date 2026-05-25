use crate::error::{RuntimeError, VmResult};
use crate::frame::{VmClosure, VmClosurePayload};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_core::MemberKey;
use varn_types::{
    value::{find_method_with_owner, BoundMethod, ClassObj, ObjRef},
    NativeCtx, Value,
};

pub fn find_getter(obj: VmValue, key: &str, heap: &Heap) -> Option<Value> {
    if !obj.is_heap() {
        let val = heap.extract(obj);
        return get_class_for_value(&val, heap)?.find_getter(key);
    }
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Object(o)) => {
            let guard = o.borrow();
            guard.class()?.find_getter(key)
        }
        Some(HeapObj::Class(cls)) => cls.find_static_getter(key),
        _ => {
            let val = heap.extract(obj);
            get_class_for_value(&val, heap)?.find_getter(key)
        }
    }
}

pub fn find_setter(obj: VmValue, key: &str, heap: &Heap) -> Option<Value> {
    if !obj.is_heap() {
        let val = heap.extract(obj);
        return get_class_for_value(&val, heap)?.find_setter(key);
    }
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Object(o)) => {
            let guard = o.borrow();
            guard.class()?.find_setter(key)
        }
        Some(HeapObj::Class(cls)) => cls.find_static_setter(key),
        _ => {
            let val = heap.extract(obj);
            get_class_for_value(&val, heap)?.find_setter(key)
        }
    }
}

pub fn get_property(obj: VmValue, key: &str, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Module(m)) = heap.get(obj.as_heap_idx()) {
            let val = m
                .export_map
                .get(key)
                .and_then(|&s| m.get_slot(s))
                .unwrap_or(VmValue::null());
            return Ok(val);
        }
    }

    if let Some(nv) = resolve_own_data_property(obj, key, heap) {
        return Ok(nv);
    }
    let val = heap.extract(obj);
    get_property_value(&val, key, heap)
        .map_err(RuntimeError::new)
        .map(|v| heap.intern(v))
}

pub fn get_property_maybe(obj: VmValue, key: &str, heap: &mut Heap) -> VmValue {
    get_property(obj, key, heap).unwrap_or(VmValue::null())
}

pub fn set_property(obj: VmValue, key: &str, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if !obj.is_heap() {
        return Err(RuntimeError::new(format!(
            "cannot set property '{}' on primitive",
            key
        )));
    }

    if matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Module(_))) {
        return Ok(());
    }
    let idx = obj.as_heap_idx();
    match heap.get(idx).cloned() {
        Some(HeapObj::Object(o)) => {
            o.borrow_mut().set_field_nv(Rc::from(key), val);
            heap.write_barrier(idx, val);
            Ok(())
        }
        Some(HeapObj::EnumVariant(ev)) => {
            if let Value::Object(o) = &ev.payload {
                o.borrow_mut().set_field_nv(Rc::from(key), val);
                heap.write_barrier(idx, val);
            }
            Ok(())
        }
        Some(HeapObj::Class(c)) => {
            let v = heap.extract(val);
            c.add_static(key, v);
            Ok(())
        }
        _ => Err(RuntimeError::new(format!(
            "cannot set property '{}': not an object",
            key
        ))),
    }
}

pub fn get_fixed_field(obj: VmValue, slot: usize, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let v = if let Some(HeapObj::Object(o)) = heap.get(obj.as_heap_idx()).cloned() {
            let g = o.borrow();
            g.inner.values.get(slot).cloned()
        } else {
            None
        };
        if let Some(v) = v {
            return Ok(v);
        }
    }
    Err(RuntimeError::new(format!(
        "OpGetFixedField: slot {} out of range",
        slot
    )))
}

pub fn set_fixed_field(obj: VmValue, slot: usize, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if obj.is_heap() {
        if let Some(HeapObj::Object(o)) = heap.get(obj.as_heap_idx()).cloned() {
            let mut g = o.borrow_mut();
            if slot < g.inner.values.len() {
                g.inner.values[slot] = val;
                heap.write_barrier(obj.as_heap_idx(), val);
                return Ok(());
            }
            return Err(RuntimeError::new(format!(
                "OpSetFixedField: slot {} out of range",
                slot
            )));
        }
    }
    Err(RuntimeError::new("OpSetFixedField: not an object"))
}

fn get_property_value(obj: &Value, key: &str, heap: &mut Heap) -> Result<Value, String> {
    if let Some(v) = resolve_intrinsic_method_property(obj, key, heap) {
        return Ok(v);
    }

    if let Some(v) = resolve_specialized_value_property(obj, key) {
        return v;
    }

    if matches!(obj, Value::Null) {
        return Err(format!("cannot read property '{}' of null", key));
    }

    Ok(Value::Null)
}

fn resolve_own_data_property(obj: VmValue, key: &str, heap: &Heap) -> Option<VmValue> {
    if !obj.is_heap() {
        return None;
    }
    let idx = obj.as_heap_idx();
    match heap.get(idx).cloned() {
        Some(HeapObj::Object(o)) => {
            let guard = o.borrow();
            guard.inner.get(key)
        }
        _ => None,
    }
}

fn get_class_for_value(val: &Value, heap: &Heap) -> Option<Rc<ClassObj>> {
    match val {
        Value::EnumVariant(ev) => heap.get_intrinsic_class(&ev.enum_name),
        Value::Object(o) => o.borrow().class(),
        Value::Class(cls) => Some(cls.clone()),
        _ => {
            let type_name = match val {
                Value::Null => "null",
                Value::Bool(_) => "bool",
                Value::Int(_) => "int",
                Value::Float(_) => "float",
                Value::Str(_) => "str",
                Value::Symbol(_) => "symbol",
                Value::BigInt(_) => "bigint",
                Value::Array(_) => "Array",
                Value::Map(_) => "Map",
                Value::Set(_) => "Set",
                Value::Range(_) => "Range",
                Value::Char(_) => "char",
                Value::Decimal(_) => "decimal",
                _ => return None,
            };
            heap.get_intrinsic_class(type_name)
        }
    }
}

fn resolve_intrinsic_method_property(obj: &Value, key: &str, heap: &mut Heap) -> Option<Value> {
    if let Value::Generator(_) = obj {
        if key == "next" {
            return Some(Value::native_bound(obj.clone(), generator_next, "next"));
        }
    }

    if matches!(obj, Value::EnumVariant(_)) && key == "name" {
        return None;
    }

    let cls = get_class_for_value(obj, heap)?;
    if let Some((method, owner)) = find_method_with_owner(&cls, key) {
        return Some(bind_method_to_receiver(obj.clone(), method, Some(owner)));
    }

    if key == "name" {
        return Some(Value::Str(Rc::from(cls.name.as_str())));
    }

    None
}

fn resolve_specialized_value_property(obj: &Value, key: &str) -> Option<Result<Value, String>> {
    match obj {
        Value::Class(cls) => {
            if let Some(v) = cls.get_static(key) {
                return Some(Ok(v));
            }
            if let Some(m) = cls.find_method(key) {
                return Some(Ok(m));
            }
            None
        }
        Value::Array(arr) => {
            if key == "length" {
                return Some(Ok(Value::Int(arr.0.borrow().len() as i64)));
            }
            if let Ok(n) = key.parse::<usize>() {
                return Some(
                    arr.0
                        .borrow()
                        .get(n)
                        .cloned()
                        .ok_or_else(|| format!("index {n} out of bounds for array")),
                );
            }
            None
        }
        Value::Str(s) => {
            if key == "length" {
                return Some(Ok(Value::Int(s.len() as i64)));
            }
            None
        }
        Value::Map(m) => {
            m.0.borrow()
                .get(&Value::Str(Rc::from(key)))
                .cloned()
                .map(Ok)
        }
        Value::Set(_) => {
            if key == "size" || key == "length" {
                None
            } else {
                None
            }
        }
        Value::EnumVariant(ev) => {
            if let Value::Object(o) = &ev.payload {
                let guard = o.borrow();
                if let Some(f) = guard.get_field(key) {
                    return Some(Ok(f));
                }
            }
            if key.starts_with("value") && key.len() > 5 {
                if let Ok(idx) = key[5..].parse::<usize>() {
                    if !ev.fields.is_empty() {
                        if idx < ev.fields.len() {
                            let field_name = &ev.fields[idx];
                            if let Value::Object(o) = &ev.payload {
                                if let Some(f) = o.borrow().get_field(field_name) {
                                    return Some(Ok(f));
                                }
                            }
                            return Some(Ok(Value::Null));
                        }
                    } else if idx == 0 {
                        return Some(Ok(ev.payload.clone()));
                    }
                }
            }
            match key {
                "__tag" => Some(Ok(Value::Int(ev.variant_tag as i64))),
                "__variant_name__" => Some(Ok(Value::Str(ev.variant_name.clone()))),
                "name" => Some(Ok(Value::Str(ev.variant_name.clone()))),
                "rawValue" => Some(Ok(Value::Int(ev.variant_tag as i64))),
                "value0" if ev.fields.is_empty() => Some(Ok(ev.payload.clone())),
                _ => None,
            }
        }
        _ => None,
    }
}

fn generator_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let gen_nv = args
        .first()
        .copied()
        .ok_or("generator.next: missing receiver")?;
    let gen_val = ctx.extract(gen_nv);
    let gen = match gen_val {
        Value::Generator(g) => g,
        _ => return Err("generator.next: invalid receiver".to_string()),
    };
    let input_nv = args.get(1).copied().unwrap_or(VmValue::null());
    let input = ctx.extract(input_nv);
    gen.0.next(input).map(|v| ctx.intern(v))
}

pub fn get_class(val: VmValue, heap: &Heap) -> Option<Rc<ClassObj>> {
    let v = heap.extract(val);
    get_class_for_value(&v, heap)
}

pub fn bind_method_to_receiver(
    receiver: Value,
    method: Value,
    _owner: Option<Rc<ClassObj>>,
) -> Value {
    match method {
        Value::VmValue(v) => {
            let bm = BoundMethod {
                receiver,
                target: varn_types::value::BoundMethodTarget::Vm {
                    closure: v,
                    owner_class: _owner,
                },
            };
            Value::BoundMethod(Box::new(bm))
        }
        Value::NativeFn(b) => {
            let (f, name) = *b;
            let bm = BoundMethod {
                receiver,
                target: varn_types::value::BoundMethodTarget::Native { func: f, name },
            };
            Value::BoundMethod(Box::new(bm))
        }
        _ => method,
    }
}
