use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_core::IntrinsicType;
use varn_types::{
    value::{find_method_with_owner, BoundMethod, ClassObj},
    NativeCtx, Value,
};

pub fn find_getter(obj: VmValue, key: &str, heap: &Heap) -> Option<Value> {
    if !obj.is_heap() {
        return get_class(obj, heap)?.find_getter(key);
    }
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Object(o)) => {
            let guard = o.borrow();
            guard.class()?.find_getter(key)
        }
        Some(HeapObj::Class(cls)) => cls.find_static_getter(key),
        // `get_class` derives the class without cloning array/string contents
        // (an `extract` here made every `arr.length` access O(n)).
        _ => get_class(obj, heap)?.find_getter(key),
    }
}

pub fn find_setter(obj: VmValue, key: &str, heap: &Heap) -> Option<Value> {
    if !obj.is_heap() {
        return get_class(obj, heap)?.find_setter(key);
    }
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Object(o)) => {
            let guard = o.borrow();
            guard.class()?.find_setter(key)
        }
        Some(HeapObj::Class(cls)) => cls.find_static_setter(key),
        _ => get_class(obj, heap)?.find_setter(key),
    }
}

pub enum ResolvedProperty {
    Nv(VmValue),
    Built(Value),
}

pub fn resolve_property(obj: VmValue, key: &str, heap: &mut Heap) -> VmResult<ResolvedProperty> {
    if obj.is_heap() {
        if let Some(HeapObj::Module(m)) = heap.get(obj.as_heap_idx()) {
            let val = m
                .export_map
                .get(key)
                .and_then(|&s| m.get_slot(s))
                .unwrap_or(VmValue::null());
            return Ok(ResolvedProperty::Nv(val));
        }
    }

    if let Some(nv) = resolve_own_data_property(obj, key, heap) {
        return Ok(ResolvedProperty::Nv(nv));
    }
    let val = heap.extract(obj);
    get_property_value(&val, key, heap)
        .map_err(RuntimeError::new)
        .map(ResolvedProperty::Built)
}

pub fn get_property(obj: VmValue, key: &str, heap: &mut Heap) -> VmResult<VmValue> {
    let resolved = resolve_property(obj, key, heap)?;
    match resolved {
        ResolvedProperty::Nv(v) => Ok(v),
        ResolvedProperty::Built(v) => Ok(heap.intern(v)),
    }
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

    if matches!(
        heap.get(obj.as_heap_idx()),
        Some(HeapObj::Module(_)) | Some(HeapObj::FrozenModule(_))
    ) {
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

#[inline(always)]
pub fn get_fixed_field(obj: VmValue, slot: usize, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let idx = obj.as_heap_idx();
        
        enum FoundField {
            Vm(VmValue),
            Val(Value),
        }

        let found = match heap.get(idx) {
            Some(HeapObj::Object(o)) => {
                let g = o.borrow();
                g.inner.values.get(slot).cloned().map(FoundField::Vm)
            }
            Some(HeapObj::EnumVariant(ev)) => {
                if let Value::Object(o) = &ev.payload {
                    let g = o.borrow();
                    g.inner.values.get(slot).cloned().map(FoundField::Vm)
                } else {
                    None
                }
            }
            Some(HeapObj::Class(cls)) => {
                let fields = cls.static_fields.borrow();
                if let Some(name) = fields.get(slot) {
                    cls.statics.borrow().get(name).cloned().map(FoundField::Val)
                } else {
                    None
                }
            }
            _ => None,
        };

        match found {
            Some(FoundField::Vm(v)) => return Ok(v),
            Some(FoundField::Val(v)) => return Ok(heap.intern(v)),
            None => {}
        }
    }
    Err(RuntimeError::new(format!(
        "OpGetFixedField: slot {} out of range",
        slot
    )))
}

#[inline(always)]
pub fn set_fixed_field(obj: VmValue, slot: usize, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        
        enum Target {
            Obj(varn_types::value::ObjRef),
            Class(Rc<ClassObj>),
        }

        let target = match heap.get(heap_idx) {
            Some(HeapObj::Object(o)) => Some(Target::Obj(o.clone())),
            Some(HeapObj::EnumVariant(ev)) => {
                if let Value::Object(o) = &ev.payload {
                    Some(Target::Obj(o.clone()))
                } else {
                    None
                }
            }
            Some(HeapObj::Class(cls)) => Some(Target::Class(cls.clone())),
            _ => None,
        };

        match target {
            Some(Target::Obj(o)) => {
                let mut g = o.borrow_mut();
                if slot < g.inner.values.len() {
                    g.inner.values[slot] = val;
                    drop(g);
                    heap.write_barrier(heap_idx, val);
                    return Ok(());
                }
            }
            Some(Target::Class(cls)) => {
                let name = {
                    let fields = cls.static_fields.borrow();
                    fields.get(slot).cloned()
                };
                if let Some(name) = name {
                    let val_extracted = heap.extract(val);
                    cls.statics.borrow_mut().insert(name, val_extracted);
                    return Ok(());
                }
            }
            None => {}
        }
    }
    Err(RuntimeError::new("OpSetFixedField: slot out of range or invalid target"))
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
        // O(1) array length. Without this, `arr.length` falls through to
        // `heap.extract(obj)` in `resolve_property`, which clones the entire
        // array's Vec just to read its length — making `arr.length` O(n) and any
        // loop over it O(n²). (`heap.get(..).cloned()` only bumps the array's Rc.)
        Some(HeapObj::Array(a)) if key == "length" => {
            Some(VmValue::from_int(a.len() as i64))
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
            let ty = match val {
                Value::Null => IntrinsicType::Null,
                Value::Bool(_) => IntrinsicType::Bool,
                Value::Int(_) => IntrinsicType::Int,
                Value::Float(_) => IntrinsicType::Float,
                Value::Str(_) => IntrinsicType::Str,
                Value::Symbol(_) => IntrinsicType::Symbol,
                Value::BigInt(_) => IntrinsicType::BigInt,
                Value::Array(_) => IntrinsicType::Array,
                Value::Map(_) => IntrinsicType::Map,
                Value::Set(_) => IntrinsicType::Set,
                Value::Range(_) => IntrinsicType::Range,
                Value::Char(_) => IntrinsicType::Char,
                Value::Decimal(_) => IntrinsicType::Decimal,
                _ => return None,
            };
            heap.get_intrinsic_class(ty.as_str())
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
                return Some(Ok(Value::Int(arr.len() as i64)));
            }
            if let Ok(n) = key.parse::<usize>() {
                return Some(
                    arr.borrow()
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
    // Fast path for heap types whose class is fixed by their type: avoid
    // `heap.extract`, which deep-clones the contents. This is on the property-
    // access IC hot path, so cloning made every `arr.length` / array property
    // access O(n) and any loop over it O(n²). These return exactly what the slow
    // path would (`get_class_for_value` maps `Value::Array` -> intrinsic "Array").
    if val.is_heap() {
        match heap.get(val.as_heap_idx()) {
            Some(HeapObj::Array(_)) => {
                return heap.get_intrinsic_class(IntrinsicType::Array.as_str())
            }
            Some(HeapObj::Str(_)) => return heap.get_intrinsic_class(IntrinsicType::Str.as_str()),
            _ => {}
        }
    }
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
