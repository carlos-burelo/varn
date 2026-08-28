use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_core::IntrinsicType;
use varn_types::{
    value::{find_method_with_owner, BoundMethod, ClassObj},
    NativeCtx, Value,
};

pub(crate) fn find_getter(obj: VmValue, key: &str, heap: &Heap) -> Option<Value> {
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

pub(crate) fn find_setter(obj: VmValue, key: &str, heap: &Heap) -> Option<Value> {
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

pub(crate) fn resolve_property(
    obj: VmValue,
    key: &str,
    heap: &mut Heap,
) -> VmResult<ResolvedProperty> {
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

    if let Some(stripped) = key.strip_prefix("::") {
        return resolve_meta_property(obj, stripped, heap);
    }

    if let Some(nv) = resolve_own_data_property(obj, key, heap) {
        return Ok(ResolvedProperty::Nv(nv));
    }
    let val = heap.extract(obj);
    get_property_value(&val, key, heap)
        .map_err(RuntimeError::new)
        .map(ResolvedProperty::Built)
}

pub(crate) fn get_property(obj: VmValue, key: &str, heap: &mut Heap) -> VmResult<VmValue> {
    let resolved = resolve_property(obj, key, heap)?;
    match resolved {
        ResolvedProperty::Nv(v) => Ok(v),
        ResolvedProperty::Built(v) => Ok(heap.intern(v)),
    }
}

pub(crate) fn get_property_maybe(obj: VmValue, key: &str, heap: &mut Heap) -> VmValue {
    get_property(obj, key, heap).unwrap_or(VmValue::null())
}

pub(crate) fn set_property(obj: VmValue, key: &str, val: VmValue, heap: &mut Heap) -> VmResult<()> {
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
            o.set_field_nv(Rc::from(key), val);
            heap.write_barrier(idx, val);
            Ok(())
        }
        Some(HeapObj::EnumVariant(ev)) => {
            if let Value::Object(o) = &ev.payload {
                o.set_field_nv(Rc::from(key), val);
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
pub(crate) fn get_fixed_field(obj: VmValue, slot: usize, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let idx = obj.as_heap_idx();
        // eprintln!("[get_fixed_field] obj={:?} idx={} (nursery={}) slot={}", obj, idx, crate::nursery::is_nursery_idx(idx), slot);

        enum FoundField {
            Vm(VmValue),
            Val(Value),
        }

        let found = match heap.get(idx) {
            Some(HeapObj::Object(o)) | Some(HeapObj::Record(o)) => {
                o.field_at(slot).map(FoundField::Vm)
            }
            Some(HeapObj::EnumVariant(ev)) => {
                if let Value::Object(o) = &ev.payload {
                    o.field_at(slot).map(FoundField::Vm)
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
        let details = if obj.is_heap() {
            match heap.get(obj.as_heap_idx()) {
                Some(HeapObj::Object(o)) => format!(
                    "Object[inline_len={}, slot_count={}, props={:?}]",
                    o.inline_len(),
                    o.slot_count(),
                    o.shape().property_names
                ),
                Some(other) => format!("{:?}", other),
                None => "None".to_string(),
            }
        } else {
            format!("{:?}", obj)
        };
        Err(RuntimeError::new(format!(
            "OpGetFixedField: slot {} out of range on obj {:?} (details: {})",
            slot, obj, details
        )))
}

#[inline(always)]
pub(crate) fn set_fixed_field(
    obj: VmValue,
    slot: usize,
    val: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
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
                if o.set_field_at(slot, val) {
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
    Err(RuntimeError::new(
        "OpSetFixedField: slot out of range or invalid target",
    ))
}

fn get_property_value(obj: &Value, key: &str, heap: &mut Heap) -> Result<Value, String> {
    if let Some(v) = resolve_intrinsic_method_property(obj, key, heap) {
        return Ok(v);
    }

    if let Some(v) = resolve_specialized_value_property(obj, key, heap) {
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
        Some(HeapObj::Object(o)) | Some(HeapObj::Record(o)) => o.get(key),
        Some(HeapObj::Array(a)) | Some(HeapObj::Tuple(a))
            if key == varn_core::MemberKey::Length.as_str() =>
        {
            Some(VmValue::from_int(a.len() as i64))
        }
        Some(HeapObj::Range(r)) => {
            if key == varn_core::MemberKey::Start.as_str() {
                Some(VmValue::from_int(r.start))
            } else if key == varn_core::MemberKey::End.as_str() {
                Some(VmValue::from_int(r.end))
            } else if key == varn_core::MemberKey::Length.as_str() {
                let len = if r.inclusive {
                    (r.end - r.start + 1).max(0)
                } else {
                    (r.end - r.start).max(0)
                };
                Some(VmValue::from_int(len))
            } else {
                None
            }
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
        if key == varn_core::MemberKey::IterNext.as_str() {
            return Some(Value::native_bound(
                obj.clone(),
                generator_next,
                varn_core::MemberKey::IterNext.as_str(),
            ));
        }
    }

    if matches!(obj, Value::EnumVariant(_)) && key == varn_core::MemberKey::Name.as_str() {
        return None;
    }

    let cls = get_class_for_value(obj, heap)?;
    if let Some((method, owner)) = find_method_with_owner(&cls, key) {
        return Some(bind_method_to_receiver(obj.clone(), method, Some(owner)));
    }

    if key == varn_core::MemberKey::Name.as_str() {
        return Some(Value::Str(Rc::from(cls.name.as_str())));
    }

    None
}

fn resolve_specialized_value_property(
    obj: &Value,
    key: &str,
    heap: &Heap,
) -> Option<Result<Value, String>> {
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
            if key == varn_core::MemberKey::Length.as_str() {
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
            if key == varn_core::MemberKey::Length.as_str() {
                return Some(Ok(Value::Int(s.len() as i64)));
            }
            None
        }
        Value::Map(m) => {
            let found = heap
                .lookup_str_map_key(key)
                .and_then(|k| m.0.borrow().get(&k).copied());
            found.map(|nv| Ok(heap.extract(nv)))
        }
        Value::Set(_) => None,
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
            use varn_core::MemberKey;
            match MemberKey::from_str(key) {
                Some(MemberKey::Tag) | Some(MemberKey::RawValue) => {
                    Some(Ok(Value::Int(ev.variant_tag)))
                }
                Some(MemberKey::VariantName) | Some(MemberKey::Name) => {
                    Some(Ok(Value::Str(ev.variant_name.clone())))
                }
                Some(MemberKey::Value0) if ev.fields.is_empty() => Some(Ok(ev.payload.clone())),
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

pub(crate) fn get_class(val: VmValue, heap: &Heap) -> Option<Rc<ClassObj>> {
    if val.is_heap() {
        match heap.get(val.as_heap_idx()) {
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => return o.borrow().class(),
            Some(HeapObj::Class(cls)) => return Some(cls.clone()),
            Some(HeapObj::Array(_) | HeapObj::Tuple(_)) => {
                return heap.get_intrinsic_class(IntrinsicType::Array.as_str())
            }
            Some(HeapObj::Str(_)) => return heap.get_intrinsic_class(IntrinsicType::Str.as_str()),
            Some(HeapObj::Map(_)) => return heap.get_intrinsic_class(IntrinsicType::Map.as_str()),
            Some(HeapObj::Set(_)) => return heap.get_intrinsic_class(IntrinsicType::Set.as_str()),
            Some(HeapObj::EnumVariant(ev)) => return heap.get_intrinsic_class(&ev.enum_name),
            Some(HeapObj::Range(_)) => {
                return heap.get_intrinsic_class(IntrinsicType::Range.as_str())
            }
            Some(HeapObj::Buffer(_)) => return heap.get_intrinsic_class("Buffer"),
            Some(HeapObj::Generator(_)) => {
                return heap.get_intrinsic_class(IntrinsicType::Generator.as_str())
            }
            _ => return None,
        }
    }
    if val.is_int() {
        return heap.get_intrinsic_class(IntrinsicType::Int.as_str());
    }
    if val.is_f64() {
        return heap.get_intrinsic_class(IntrinsicType::Float.as_str());
    }
    if val.is_bool() {
        return heap.get_intrinsic_class(IntrinsicType::Bool.as_str());
    }
    if val.is_sso() {
        return heap.get_intrinsic_class(IntrinsicType::Str.as_str());
    }
    if val.is_null() {
        return heap.get_intrinsic_class(IntrinsicType::Null.as_str());
    }
    None
}

pub(crate) fn bind_method_to_receiver(
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

pub(crate) fn resolve_meta_property(
    obj: VmValue,
    meta_key: &str,
    heap: &mut Heap,
) -> VmResult<ResolvedProperty> {
    use varn_core::MemberKey;
    let val = heap.extract(obj);
    let key_enum = MemberKey::from_str(meta_key);
    match key_enum {
        Some(MemberKey::Type) => {
            let type_str: &str = match &val {
                Value::Int(_) => IntrinsicType::Int.as_str(),
                Value::Float(_) => IntrinsicType::Float.as_str(),
                Value::Str(_) => IntrinsicType::Str.as_str(),
                Value::Bool(_) => IntrinsicType::Bool.as_str(),
                Value::Char(_) => IntrinsicType::Char.as_str(),
                Value::Null => IntrinsicType::Null.as_str(),
                Value::Array(_) => IntrinsicType::Array.as_str(),
                Value::Map(_) => IntrinsicType::Map.as_str(),
                Value::Set(_) => IntrinsicType::Set.as_str(),
                Value::Class(cls) => cls.name.as_str(),
                Value::Object(o) => {
                    if let Some(c) = o.0.class() {
                        return Ok(ResolvedProperty::Built(Value::Str(c.name.clone().into())));
                    } else {
                        "Object"
                    }
                }
                Value::EnumVariant(ev) => ev.enum_name.as_ref(),
                Value::Decimal(_) => IntrinsicType::Decimal.as_str(),
                Value::BigInt(_) => IntrinsicType::BigInt.as_str(),
                Value::Generator(_) => IntrinsicType::Generator.as_str(),
                Value::Task(_) | Value::TaskHandle(_) => IntrinsicType::Task.as_str(),
                Value::Range(_) => IntrinsicType::Range.as_str(),
                Value::Symbol(_) => IntrinsicType::Symbol.as_str(),
                Value::NativeFn(_) | Value::BoundMethod(_) | Value::VmValue(_) => "Function",
                _ => IntrinsicType::Dynamic.as_str(),
            };
            Ok(ResolvedProperty::Built(Value::Str(Rc::from(type_str))))
        }
        Some(MemberKey::Name) => {
            let name_val = match &val {
                Value::Class(cls) => Value::Str(Rc::from(cls.name.as_str())),
                Value::EnumVariant(ev) => Value::Str(ev.variant_name.clone()),
                Value::Object(o) => {
                    if let Some(c) = o.0.class() {
                        Value::Str(Rc::from(c.name.as_str()))
                    } else {
                        Value::Str(Rc::from("Object"))
                    }
                }
                _ => Value::Null,
            };
            Ok(ResolvedProperty::Built(name_val))
        }
        Some(MemberKey::Class) => {
            let class_val = match &val {
                Value::Object(o) => o.0.class().map(Value::Class).unwrap_or(Value::Null),
                Value::Class(cls) => Value::Class(cls.clone()),
                _ => get_class_for_value(&val, heap)
                    .map(Value::Class)
                    .unwrap_or(Value::Null),
            };
            Ok(ResolvedProperty::Built(class_val))
        }
        Some(MemberKey::Fields) => {
            let fields: Vec<Value> = match &val {
                Value::Class(cls) => {
                    let shape = cls.root_shape.borrow();
                    let mut pairs: Vec<(Rc<str>, usize)> = shape
                        .property_names
                        .iter()
                        .map(|(k, &idx)| (Rc::clone(k), idx))
                        .collect();
                    pairs.sort_unstable_by_key(|(_, idx)| *idx);
                    pairs.into_iter().map(|(k, _)| Value::Str(k)).collect()
                }
                Value::Object(o) => o.0.keys().map(Value::Str).collect(),
                _ => Vec::new(),
            };
            Ok(ResolvedProperty::Built(Value::Array(
                varn_types::value::ArrayRef::new(fields),
            )))
        }
        Some(MemberKey::Methods) => {
            let methods: Vec<Value> = match &val {
                Value::Class(cls) => cls
                    .method_map
                    .borrow()
                    .keys()
                    .map(|k| Value::Str(Rc::clone(k)))
                    .collect(),
                Value::Object(o) => o
                    .0
                    .class()
                    .map(|c| {
                        c.method_map
                            .borrow()
                            .keys()
                            .map(|k| Value::Str(Rc::clone(k)))
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            Ok(ResolvedProperty::Built(Value::Array(
                varn_types::value::ArrayRef::new(methods),
            )))
        }
        Some(MemberKey::Keys) => Ok(ResolvedProperty::Built(Value::native_bound(
            val,
            meta_keys_native,
            MemberKey::Keys.as_str(),
        ))),
        Some(MemberKey::Values) => Ok(ResolvedProperty::Built(Value::native_bound(
            val,
            meta_values_native,
            MemberKey::Values.as_str(),
        ))),
        Some(MemberKey::Entries) => Ok(ResolvedProperty::Built(Value::native_bound(
            val,
            meta_entries_native,
            MemberKey::Entries.as_str(),
        ))),
        Some(MemberKey::HasOwn) => Ok(ResolvedProperty::Built(Value::native_bound(
            val,
            meta_has_own_native,
            MemberKey::HasOwn.as_str(),
        ))),
        _ => Ok(ResolvedProperty::Built(Value::Null)),
    }
}

fn meta_keys_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let recv = args.first().copied().ok_or("meta.keys: missing receiver")?;
    let val = ctx.extract(recv);
    let keys: Vec<Value> = match &val {
        Value::Object(o) => o.0.keys().map(Value::Str).collect(),
        Value::Class(cls) => {
            let shape = cls.root_shape.borrow();
            let mut pairs: Vec<(Rc<str>, usize)> = shape
                .property_names
                .iter()
                .map(|(k, &idx)| (Rc::clone(k), idx))
                .collect();
            pairs.sort_unstable_by_key(|(_, idx)| *idx);
            pairs.into_iter().map(|(k, _)| Value::Str(k)).collect()
        }
        Value::Map(m) => m.0.borrow().keys().map(|k| ctx.extract(k.0)).collect(),
        _ => Vec::new(),
    };
    Ok(ctx.intern(Value::Array(varn_types::value::ArrayRef::new(keys))))
}

fn meta_values_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let recv = args
        .first()
        .copied()
        .ok_or("meta.values: missing receiver")?;
    let val = ctx.extract(recv);
    let values: Vec<Value> = match &val {
        Value::Object(o) => o
            .0
            .keys()
            .filter_map(|k| o.0.get(&k).map(|nv| ctx.extract(nv)))
            .collect(),
        Value::Array(a) => a.read().clone(),
        Value::Map(m) => m.0.borrow().values().map(|v| ctx.extract(*v)).collect(),
        _ => Vec::new(),
    };
    Ok(ctx.intern(Value::Array(varn_types::value::ArrayRef::new(values))))
}

fn meta_entries_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let recv = args
        .first()
        .copied()
        .ok_or("meta.entries: missing receiver")?;
    let val = ctx.extract(recv);
    let entries: Vec<Value> = match &val {
        Value::Object(o) => o
            .0
            .keys()
            .filter_map(|k| {
                let v = o.0.get(&k).map(|nv| ctx.extract(nv))?;
                Some(Value::Array(varn_types::value::ArrayRef::new(vec![
                    Value::Str(k),
                    v,
                ])))
            })
            .collect(),
        Value::Array(a) => a
            .read()
            .iter()
            .enumerate()
            .map(|(i, v)| {
                Value::Array(varn_types::value::ArrayRef::new(vec![
                    Value::Int(i as i64),
                    v.clone(),
                ]))
            })
            .collect(),
        Value::Map(m) => m
            .0
            .borrow()
            .iter()
            .map(|(k, v)| {
                Value::Array(varn_types::value::ArrayRef::new(vec![
                    ctx.extract(k.0),
                    ctx.extract(*v),
                ]))
            })
            .collect(),
        _ => Vec::new(),
    };
    Ok(ctx.intern(Value::Array(varn_types::value::ArrayRef::new(entries))))
}

fn meta_has_own_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    let recv = args
        .first()
        .copied()
        .ok_or("meta.hasOwn: missing receiver")?;
    let key_nv = args.get(1).copied().unwrap_or(VmValue::null());
    let key_val = ctx.extract(key_nv);
    let key_str = match &key_val {
        Value::Str(s) => s.as_ref(),
        _ => return Ok(VmValue::from_bool(false)),
    };
    let val = ctx.extract(recv);
    let exists = match &val {
        Value::Object(o) => o.0.contains_key(key_str),
        Value::Map(m) => m.0.borrow().keys().any(|k| match ctx.extract(k.0) {
            Value::Str(s) => s.as_ref() == key_str,
            _ => false,
        }),
        _ => false,
    };
    Ok(VmValue::from_bool(exists))
}


