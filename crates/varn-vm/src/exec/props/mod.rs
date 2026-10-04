mod fixed;
pub(crate) mod meta;

pub(crate) use fixed::{get_fixed_field, get_fixed_field_at, set_fixed_field, set_fixed_field_at};
pub(crate) use meta::resolve_meta_property;

use std::rc::Rc;

use varn_core::MemberKey;
use varn_types::value::{find_method_with_owner, ClassObj, ObjRef};
use varn_types::NativeCtx;

use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapInner, HeapObj};
use crate::value::VmValue;

pub(crate) fn find_getter(obj: VmValue, key: &str, heap: &Heap) -> Option<VmValue> {
    if !obj.is_heap() {
        return get_class(obj, heap)?.find_getter(key);
    }
    match heap.get(obj.as_heap()) {
        Some(HeapObj::Object(o)) => o.borrow().class()?.find_getter(key),
        Some(HeapObj::Class(cls)) => cls.find_static_getter(key),
        _ => get_class(obj, heap)?.find_getter(key),
    }
}

pub(crate) fn find_setter(obj: VmValue, key: &str, heap: &Heap) -> Option<VmValue> {
    if !obj.is_heap() {
        return get_class(obj, heap)?.find_setter(key);
    }
    match heap.get(obj.as_heap()) {
        Some(HeapObj::Object(o)) => o.borrow().class()?.find_setter(key),
        Some(HeapObj::Class(cls)) => cls.find_static_setter(key),
        _ => get_class(obj, heap)?.find_setter(key),
    }
}

pub(crate) fn payload_object(heap: &Heap, payload: VmValue) -> Option<ObjRef> {
    if !payload.is_heap() {
        return None;
    }
    match heap.get(payload.as_heap()) {
        Some(HeapObj::Object(o) | HeapObj::Record(o)) => Some(o.clone()),
        _ => None,
    }
}

pub(crate) fn get_property(obj: VmValue, key: &str, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Module(m)) = heap.get(obj.as_heap()) {
            return Ok(m
                .export_map
                .get(key)
                .and_then(|&s| m.get_slot(s))
                .unwrap_or(VmValue::null()));
        }
    }
    if let Some(stripped) = key.strip_prefix("::") {
        return resolve_meta_property(obj, stripped, heap);
    }
    if let Some(nv) = resolve_own_data_property(obj, key, heap) {
        return Ok(nv);
    }
    if let Some(v) = resolve_intrinsic_method_property(obj, key, heap) {
        return Ok(v);
    }
    if let Some(v) = resolve_specialized_property(obj, key, heap) {
        return v.map_err(RuntimeError::new);
    }
    if obj.is_null() {
        return Err(RuntimeError::new(format!(
            "cannot read property '{}' of null",
            key
        )));
    }
    Ok(VmValue::null())
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
        heap.get(obj.as_heap()),
        Some(HeapObj::Module(_)) | Some(HeapObj::FrozenModule(_))
    ) {
        return Ok(());
    }
    let idx = obj.as_heap();
    match heap.get(idx).cloned() {
        Some(HeapObj::Instance(inst)) => {
            let cls = ClassObj::find_by_id(inst.class_id);
            let layout = cls.as_ref().map(|c| c.get_or_compute_layout());
            let field = layout.as_ref().and_then(|l| l.get_field(key));
            if let Some(f) = field {
                if inst.write_field(f, val).is_ok() {
                    heap.write_barrier(idx, val);
                    return Ok(());
                }
            }
            Err(RuntimeError::new(format!(
                "cannot set property '{}': no such field on class {:?}",
                key,
                cls.map(|c| c.name.clone())
            )))
        }
        Some(HeapObj::Object(o)) => {
            o.set_field_str(key, val);
            heap.write_barrier(idx, val);
            Ok(())
        }
        Some(HeapObj::EnumVariant(ev)) => {
            if let Some(o) = payload_object(heap, ev.payload) {
                o.set_field_str(key, val);
                heap.write_barrier(ev.payload.as_heap(), val);
            }
            Ok(())
        }
        Some(HeapObj::Class(c)) => {
            c.add_static(key, val);
            Ok(())
        }
        _ => Err(RuntimeError::new(format!(
            "cannot set property '{}': not an object",
            key
        ))),
    }
}

fn resolve_own_data_property(obj: VmValue, key: &str, heap: &Heap) -> Option<VmValue> {
    if !obj.is_heap() {
        return None;
    }
    match heap.get(obj.as_heap()).cloned() {
        Some(HeapObj::Instance(inst)) => {
            let cls = ClassObj::find_by_id(inst.class_id)?;
            let layout = cls.get_or_compute_layout();
            let f = layout.get_field(key)?;
            inst.read_field(f)
        }
        Some(HeapObj::Object(o)) | Some(HeapObj::Record(o)) => o.get(key),
        Some(HeapObj::Array(a)) | Some(HeapObj::Tuple(a)) if key == MemberKey::Length.as_str() => {
            Some(VmValue::from_int(a.len() as i64))
        }
        Some(HeapObj::Buffer(b)) if key == MemberKey::Length.as_str() => {
            Some(VmValue::from_int(b.len() as i64))
        }
        Some(HeapObj::Range(r)) => {
            let int_bounds = r.elem == varn_types::value::RangeElem::Int;
            if int_bounds && key == MemberKey::Start.as_str() {
                Some(VmValue::from_int(r.start))
            } else if int_bounds && key == MemberKey::End.as_str() {
                Some(VmValue::from_int(r.end))
            } else if key == MemberKey::Length.as_str() {
                Some(VmValue::from_int(r.len()))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn class_for_property(val: VmValue, heap: &Heap) -> Option<Rc<ClassObj>> {
    if let Some(cls) = get_class(val, heap) {
        return Some(cls);
    }
    if !val.is_heap() {
        return None;
    }
    let kind = match heap.get(val.as_heap())? {
        HeapObj::Symbol(_) => varn_core::RuntimeKind::Symbol,
        HeapObj::BigInt(_) => varn_core::RuntimeKind::BigInt,
        HeapObj::Decimal(_) => varn_core::RuntimeKind::Decimal,
        HeapObj::Char(_) => varn_core::RuntimeKind::Char,
        _ => return None,
    };
    heap.get_intrinsic_class(kind.name())
}

fn resolve_intrinsic_method_property(obj: VmValue, key: &str, heap: &mut Heap) -> Option<VmValue> {
    if obj.is_heap() {
        match heap.get(obj.as_heap()) {
            Some(HeapObj::Generator(_)) if key == MemberKey::IterNext.as_str() => {
                return Some(heap.alloc_bound_native(
                    obj,
                    generator_next,
                    MemberKey::IterNext.as_str(),
                ));
            }
            Some(HeapObj::EnumVariant(_)) if key == MemberKey::Name.as_str() => return None,
            _ => {}
        }
    }
    let cls = class_for_property(obj, heap)?;
    if let Some((method, owner)) = find_method_with_owner(&cls, key) {
        return Some(bind_method_to_receiver(heap, obj, method, Some(owner)));
    }
    if key == MemberKey::Name.as_str() {
        return Some(HeapInner::alloc_str(heap, cls.name.as_str()));
    }
    None
}

fn resolve_specialized_property(
    obj: VmValue,
    key: &str,
    heap: &mut Heap,
) -> Option<Result<VmValue, String>> {
    if obj.is_sso() {
        if key == MemberKey::Length.as_str() {
            return Some(Ok(VmValue::from_int(heap.str_repr(obj).len() as i64)));
        }
        return None;
    }
    if !obj.is_heap() {
        return None;
    }
    match heap.get(obj.as_heap()).cloned() {
        Some(HeapObj::Class(cls)) => cls.get_static(key).or_else(|| cls.find_method(key)).map(Ok),
        Some(HeapObj::Array(arr)) => {
            if key == MemberKey::Length.as_str() {
                return Some(Ok(VmValue::from_int(arr.len() as i64)));
            }
            let n = key.parse::<usize>().ok()?;
            Some(
                arr.get_vm(n)
                    .ok_or_else(|| format!("index {n} out of bounds for array")),
            )
        }
        Some(HeapObj::Str(_)) => {
            if key == MemberKey::Length.as_str() {
                return Some(Ok(VmValue::from_int(heap.str_repr(obj).len() as i64)));
            }
            None
        }
        Some(HeapObj::Buffer(b)) => {
            if key == MemberKey::Length.as_str() {
                return Some(Ok(VmValue::from_int(b.len() as i64)));
            }
            None
        }
        Some(HeapObj::Map(m)) => {
            let found = heap
                .lookup_str_map_key(key)
                .and_then(|k| m.0.borrow().get(&k).copied());
            found.map(Ok)
        }
        Some(HeapObj::EnumVariant(ev)) => enum_variant_property(&ev, key, heap),
        _ => None,
    }
}

fn enum_variant_property(
    ev: &varn_types::value::EnumVariantData,
    key: &str,
    heap: &mut Heap,
) -> Option<Result<VmValue, String>> {
    let payload = payload_object(heap, ev.payload);
    if let Some(f) = payload.as_ref().and_then(|o| o.get(key)) {
        return Some(Ok(f));
    }
    if key.starts_with("value") && key.len() > 5 {
        if let Ok(idx) = key[5..].parse::<usize>() {
            if !ev.fields.is_empty() {
                if idx < ev.fields.len() {
                    let field = payload.as_ref().and_then(|o| o.get(&ev.fields[idx]));
                    return Some(Ok(field.unwrap_or(VmValue::null())));
                }
            } else if idx == 0 {
                return Some(Ok(ev.payload));
            }
        }
    }
    match MemberKey::from_str(key) {
        Some(MemberKey::Tag) | Some(MemberKey::RawValue) => {
            Some(Ok(VmValue::from_int(ev.variant_tag)))
        }
        Some(MemberKey::VariantName) | Some(MemberKey::Name) => {
            Some(Ok(HeapInner::alloc_str(heap, &*ev.variant_name)))
        }
        Some(MemberKey::Value0) if ev.fields.is_empty() => Some(Ok(ev.payload)),
        _ => None,
    }
}

fn generator_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let gen_nv = args
        .first()
        .copied()
        .ok_or("generator.next: missing receiver")?;
    let gen = ctx
        .as_generator(gen_nv)
        .ok_or("generator.next: invalid receiver")?;
    let input = args.get(1).copied().unwrap_or(VmValue::null());
    gen.0.next(input).map_err(Into::into)
}

pub(crate) fn get_class(val: VmValue, heap: &Heap) -> Option<Rc<ClassObj>> {
    if val.is_heap() {
        let intrinsic = |kind: varn_core::RuntimeKind| heap.get_intrinsic_class(kind.name());
        return match heap.get(val.as_heap()) {
            Some(HeapObj::Instance(inst)) => ClassObj::find_by_id(inst.class_id),
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => o.borrow().class(),
            Some(HeapObj::Class(cls)) => Some(cls.clone()),
            Some(HeapObj::Array(_) | HeapObj::Tuple(_)) => intrinsic(varn_core::RuntimeKind::Array),
            Some(HeapObj::Str(_)) => intrinsic(varn_core::RuntimeKind::Str),
            Some(HeapObj::Map(_)) => intrinsic(varn_core::RuntimeKind::Map),
            Some(HeapObj::Set(_)) => intrinsic(varn_core::RuntimeKind::Set),
            Some(HeapObj::EnumVariant(ev)) => ev.enum_class_id.and_then(ClassObj::find_by_id),
            Some(HeapObj::Range(_)) => intrinsic(varn_core::RuntimeKind::Range),
            Some(HeapObj::Buffer(_)) => intrinsic(varn_core::RuntimeKind::Bytes),
            Some(HeapObj::Generator(_)) => intrinsic(varn_core::RuntimeKind::Generator),
            _ => None,
        };
    }
    let kind = if val.is_int() {
        varn_core::RuntimeKind::Int
    } else if val.is_f64() {
        varn_core::RuntimeKind::Float
    } else if val.is_bool() {
        varn_core::RuntimeKind::Bool
    } else if val.is_sso() {
        varn_core::RuntimeKind::Str
    } else if val.is_null() {
        varn_core::RuntimeKind::Null
    } else {
        return None;
    };
    heap.get_intrinsic_class(kind.name())
}

pub(crate) fn bind_method_to_receiver(
    heap: &mut Heap,
    receiver: VmValue,
    method: VmValue,
    owner: Option<Rc<ClassObj>>,
) -> VmValue {
    if !method.is_heap() {
        return method;
    }
    match heap.get(method.as_heap()) {
        Some(HeapObj::VmClosure(_)) => heap.alloc_bound_vm(receiver, method, owner),
        Some(HeapObj::NativeFn(f, name)) => {
            let (f, name) = (*f, *name);
            heap.alloc_bound_native(receiver, f, name)
        }
        _ => method,
    }
}
