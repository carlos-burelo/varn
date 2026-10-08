use varn_core::MemberKey;
use varn_types::value::{ClassObj, ObjRef};

use super::intrinsic::{get_class, resolve_intrinsic_method_property};
use super::meta::resolve_meta_property;
use super::specialized::resolve_specialized_property;
use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

pub(crate) fn find_getter(obj: VmValue, key: &str, heap: &Heap) -> Option<VmValue> {
    if !obj.is_heap() {
        return get_class(obj, heap)?.find_getter(key);
    }
    match heap.get(obj.as_heap()) {
        Some(HeapObj::Object(o)) => o.borrow().class()?.find_getter(key),
        Some(HeapObj::Class(cls)) => cls.find_static_getter(key),
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => get_class(obj, heap)?.find_getter(key),
    }
}

pub(crate) fn find_setter(obj: VmValue, key: &str, heap: &Heap) -> Option<VmValue> {
    if !obj.is_heap() {
        return get_class(obj, heap)?.find_setter(key);
    }
    match heap.get(obj.as_heap()) {
        Some(HeapObj::Object(o)) => o.borrow().class()?.find_setter(key),
        Some(HeapObj::Class(cls)) => cls.find_static_setter(key),
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => get_class(obj, heap)?.find_setter(key),
    }
}

pub(crate) fn payload_object(heap: &Heap, payload: VmValue) -> Option<ObjRef> {
    if !payload.is_heap() {
        return None;
    }
    match heap.get(payload.as_heap()) {
        Some(HeapObj::Object(o) | HeapObj::Record(o)) => Some(*o),
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::Class(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => None,
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
    if let Some(inst) = heap.instance(idx) {
        let cls = ClassObj::find_by_id(inst.class_id);
        let layout = cls.as_ref().map(|c| c.layout());
        let field = layout.as_ref().and_then(|l| l.get_field(key));
        if let Some(f) = field {
            if inst.write_field(f, val).is_ok() {
                heap.write_barrier(idx, val);
                return Ok(());
            }
        }
        return Err(RuntimeError::new(format!(
            "cannot set property '{}': no such field on class {:?}",
            key,
            cls.map(|c| c.name.clone())
        )));
    }
    match heap.get(idx).cloned() {
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
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => Err(RuntimeError::new(format!(
            "cannot set property '{}': not an object",
            key
        ))),
    }
}

pub(super) fn resolve_own_data_property(obj: VmValue, key: &str, heap: &Heap) -> Option<VmValue> {
    if !obj.is_heap() {
        return None;
    }
    if let Some(inst) = heap.instance(obj.as_heap()) {
        let cls = ClassObj::find_by_id(inst.class_id)?;
        let layout = cls.layout();
        let f = layout.get_field(key)?;
        return inst.read_field(f);
    }
    match heap.get(obj.as_heap()).cloned() {
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
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::Class(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => None,
    }
}
