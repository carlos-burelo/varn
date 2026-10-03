use std::rc::Rc;
use std::sync::Arc;

use varn_core::{MemberKey, RuntimeKind};
use varn_types::value::{ClassObj, ObjRef};
use varn_types::NativeCtx;

use super::get_class;
use crate::error::VmResult;
use crate::heap::{Heap, HeapInner, HeapObj};
use crate::value::VmValue;

fn str_array(heap: &mut Heap, names: impl Iterator<Item = Arc<str>>) -> VmValue {
    let items = names.map(|n| HeapInner::alloc_str(heap, &n)).collect();
    heap.alloc_array_vm(items)
}

fn class_field_names(cls: &ClassObj) -> Vec<Arc<str>> {
    cls.root_shape.borrow().ordered_names().to_vec()
}

fn class_method_names(cls: &ClassObj) -> Vec<Arc<str>> {
    cls.method_map.borrow().keys().cloned().collect()
}

fn class_of(obj: VmValue, heap: &Heap) -> Option<Rc<ClassObj>> {
    if !obj.is_heap() {
        return None;
    }
    match heap.get(obj.as_heap_idx())? {
        HeapObj::Instance(_) | HeapObj::Object(_) | HeapObj::Record(_) | HeapObj::Class(_) => {
            get_class(obj, heap)
        }
        _ => None,
    }
}

pub(crate) fn type_name(obj: VmValue, heap: &Heap) -> String {
    if obj.is_int() {
        return RuntimeKind::Int.name().into();
    }
    if obj.is_f64() {
        return RuntimeKind::Float.name().into();
    }
    if obj.is_bool() {
        return RuntimeKind::Bool.name().into();
    }
    if obj.is_sso() {
        return RuntimeKind::Str.name().into();
    }
    if obj.is_null() {
        return RuntimeKind::Null.name().into();
    }
    match obj.is_heap().then(|| heap.get(obj.as_heap_idx())).flatten() {
        Some(HeapObj::Class(cls)) => cls.name.as_str().into(),
        Some(HeapObj::Instance(_) | HeapObj::Object(_) | HeapObj::Record(_)) => {
            class_of(obj, heap).map_or_else(|| "Object".into(), |c| c.name.as_str().into())
        }
        Some(HeapObj::EnumVariant(ev)) => ev.enum_name.to_string(),
        Some(other) => other.tag().name().into(),
        None => varn_core::LangPrimitive::Dynamic.name().into(),
    }
}

fn snapshot_object(heap: &mut Heap, pairs: Vec<(Arc<str>, VmValue)>) -> VmValue {
    let obj = ObjRef::from_pairs(pairs);
    VmValue::from_heap_idx(heap.alloc(HeapObj::Object(obj)))
}

fn instance_snapshot(obj: VmValue, cls: &ClassObj, heap: &mut Heap) -> VmValue {
    let pairs = match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Instance(inst)) => cls
            .get_or_compute_layout()
            .fields
            .iter()
            .filter_map(|f| inst.read_field(f).map(|v| (Arc::clone(&f.name), v)))
            .collect(),
        _ => Vec::new(),
    };
    snapshot_object(heap, pairs)
}

fn class_snapshot(cls: &ClassObj, heap: &mut Heap) -> VmValue {
    let pairs = class_field_names(cls)
        .into_iter()
        .map(|n| (n, VmValue::null()))
        .collect();
    snapshot_object(heap, pairs)
}

pub(crate) fn resolve_meta_property(
    obj: VmValue,
    meta_key: &str,
    heap: &mut Heap,
) -> VmResult<VmValue> {
    let Some(key) = MemberKey::from_str(meta_key) else {
        return Ok(VmValue::null());
    };
    let cls = class_of(obj, heap);
    let is_instance =
        obj.is_heap() && matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Instance(_)));
    let is_class = obj.is_heap() && matches!(heap.get(obj.as_heap_idx()), Some(HeapObj::Class(_)));
    match key {
        MemberKey::Type => {
            let name = type_name(obj, heap);
            Ok(HeapInner::alloc_str(heap, &name))
        }
        MemberKey::Name => {
            let name: Option<String> =
                match obj.is_heap().then(|| heap.get(obj.as_heap_idx())).flatten() {
                    Some(HeapObj::EnumVariant(ev)) => Some(ev.variant_name.to_string()),
                    Some(
                        HeapObj::Instance(_)
                        | HeapObj::Object(_)
                        | HeapObj::Record(_)
                        | HeapObj::Class(_),
                    ) => Some(
                        cls.as_ref()
                            .map_or_else(|| "Object".into(), |c| c.name.as_str().into()),
                    ),
                    _ => None,
                };
            Ok(name.map_or(VmValue::null(), |n| HeapInner::alloc_str(heap, &n)))
        }
        MemberKey::Class => {
            let found = if is_class { None } else { get_class(obj, heap) };
            let target = if is_class {
                Some(obj)
            } else {
                found.map(|c| heap.alloc_class_vm(c))
            };
            Ok(target.unwrap_or(VmValue::null()))
        }
        MemberKey::Fields => {
            let names = match (
                &cls,
                obj.is_heap().then(|| heap.get(obj.as_heap_idx())).flatten(),
            ) {
                (Some(c), Some(HeapObj::Instance(_) | HeapObj::Class(_))) => class_field_names(c),
                (_, Some(HeapObj::Object(o) | HeapObj::Record(o))) => o.keys().collect(),
                _ => Vec::new(),
            };
            Ok(str_array(heap, names.into_iter()))
        }
        MemberKey::Methods => {
            let names = cls.as_deref().map(class_method_names).unwrap_or_default();
            Ok(str_array(heap, names.into_iter()))
        }
        MemberKey::Keys | MemberKey::Values | MemberKey::Entries | MemberKey::HasOwn => {
            let native = match key {
                MemberKey::Keys => meta_keys_native,
                MemberKey::Values => meta_values_native,
                MemberKey::Entries => meta_entries_native,
                _ => meta_has_own_native,
            };
            let receiver = match (&cls, is_instance, is_class) {
                (Some(c), true, _) => instance_snapshot(obj, c, heap),
                (Some(c), _, true) => class_snapshot(c, heap),
                _ => obj,
            };
            Ok(heap.alloc_bound_native(receiver, native, key.as_str()))
        }
        _ => Ok(VmValue::null()),
    }
}

fn meta_keys_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let recv = args.first().copied().ok_or("meta.keys: missing receiver")?;
    let mut keys: Vec<VmValue> = Vec::new();
    if ctx.is_object(recv) {
        let mut names: Vec<String> = Vec::new();
        ctx.object_for_each(recv, &mut |k, _| names.push(k.to_owned()));
        keys.extend(names.into_iter().map(|n| ctx.alloc_str_owned(n)));
    } else {
        ctx.map_for_each(recv, &mut |k, _| keys.push(k));
    }
    Ok(ctx.alloc_array(keys))
}

fn meta_values_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let recv = args
        .first()
        .copied()
        .ok_or("meta.values: missing receiver")?;
    let mut values: Vec<VmValue> = Vec::new();
    if ctx.is_object(recv) {
        ctx.object_for_each(recv, &mut |_, v| values.push(v));
    } else if ctx.is_array(recv) {
        ctx.array_for_each(recv, &mut |v, _| values.push(v));
    } else {
        ctx.map_for_each(recv, &mut |_, v| values.push(v));
    }
    Ok(ctx.alloc_array(values))
}

fn meta_entries_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let recv = args
        .first()
        .copied()
        .ok_or("meta.entries: missing receiver")?;
    let mut pairs: Vec<(Option<String>, VmValue, VmValue)> = Vec::new();
    if ctx.is_object(recv) {
        ctx.object_for_each(recv, &mut |k, v| {
            pairs.push((Some(k.to_owned()), VmValue::null(), v))
        });
    } else if ctx.is_array(recv) {
        ctx.array_for_each(recv, &mut |v, i| {
            pairs.push((None, VmValue::from_int(i as i64), v))
        });
    } else {
        ctx.map_for_each(recv, &mut |k, v| pairs.push((None, k, v)));
    }
    let mut entries = Vec::with_capacity(pairs.len());
    for (name, key, value) in pairs {
        let key = match name {
            Some(n) => ctx.alloc_str_owned(n),
            None => key,
        };
        entries.push(ctx.alloc_array(vec![key, value]));
    }
    Ok(ctx.alloc_array(entries))
}

fn meta_has_own_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> varn_types::NativeFnResult {
    let recv = args
        .first()
        .copied()
        .ok_or("meta.hasOwn: missing receiver")?;
    let key = args.get(1).copied().unwrap_or(VmValue::null());
    let Some(key_str) = ctx.str_owned(key) else {
        return Ok(VmValue::from_bool(false));
    };
    let exists = if ctx.is_object(recv) {
        ctx.get_field(recv, &key_str).is_some()
    } else {
        let mut found = false;
        ctx.map_for_each(recv, &mut |k, _| {
            found |= ctx.str_owned(k).is_some_and(|s| s == key_str)
        });
        found
    };
    Ok(VmValue::from_bool(exists))
}
