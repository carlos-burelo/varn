use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::TypeDesc;
use varn_types::value::ValueMap;

struct Scope<'a> {
    desc: &'a TypeDesc,
    parent: Option<&'a Scope<'a>>,
}

fn reshapes(desc: &TypeDesc) -> bool {
    match desc {
        TypeDesc::Map(..) => true,
        TypeDesc::Any
        | TypeDesc::Int
        | TypeDesc::Float
        | TypeDesc::Bool
        | TypeDesc::Char
        | TypeDesc::Str
        | TypeDesc::Bytes
        | TypeDesc::Decimal
        | TypeDesc::BigInt
        | TypeDesc::Callable
        | TypeDesc::Class { .. }
        | TypeDesc::Enum { .. }
        | TypeDesc::Enclosing { .. } => false,
        TypeDesc::Array(inner)
        | TypeDesc::Set(inner)
        | TypeDesc::Dict(inner)
        | TypeDesc::Nullable(inner) => reshapes(inner),
        TypeDesc::Tuple(_) => true,
        TypeDesc::Object(fields) => fields.iter().any(|field| reshapes(&field.ty)),
        TypeDesc::Args { fixed, rest } => {
            fixed.iter().any(reshapes) || rest.as_deref().is_some_and(reshapes)
        }
    }
}

pub(crate) fn converts(desc: &TypeDesc) -> bool {
    reshapes(desc)
}

pub(crate) fn normalize(heap: &mut Heap, value: VmValue, desc: &TypeDesc) -> VmValue {
    if !reshapes(desc) {
        return value;
    }
    let mut visiting = Vec::new();
    walk(heap, value, desc, None, &mut visiting)
}

pub(crate) fn object_entries(
    heap: &crate::heap::structs::HeapInner,
    v: VmValue,
) -> Option<Vec<(String, VmValue)>> {
    if !v.is_heap() {
        return None;
    }
    let Some(HeapObj::Object(obj) | HeapObj::Record(obj)) = heap.get(v.as_heap()) else {
        return None;
    };
    let obj = obj.borrow();
    let mut named: Vec<(usize, String)> = obj
        .shape()
        .property_names
        .iter()
        .map(|(name, slot)| (*slot, name.to_string()))
        .collect();
    named.sort_by_key(|(slot, _)| *slot);
    Some(
        named
            .into_iter()
            .filter_map(|(slot, name)| Some((name, obj.field_at(slot)?)))
            .collect(),
    )
}

pub(crate) fn elements(heap: &Heap, v: VmValue) -> Option<Vec<VmValue>> {
    if !v.is_heap() {
        return None;
    }
    let Some(HeapObj::Array(arr) | HeapObj::Tuple(arr)) = heap.get(v.as_heap()) else {
        return None;
    };
    Some((0..arr.len()).filter_map(|i| arr.get_vm(i)).collect())
}

pub(crate) fn store_element(heap: &Heap, v: VmValue, index: usize, item: VmValue) {
    if let Some(HeapObj::Array(arr) | HeapObj::Tuple(arr)) = heap.get(v.as_heap()) {
        arr.set_vm(index, item);
    }
}

fn walk(
    heap: &mut Heap,
    v: VmValue,
    desc: &TypeDesc,
    scope: Option<&Scope>,
    visiting: &mut Vec<(u64, *const TypeDesc)>,
) -> VmValue {
    match desc {
        TypeDesc::Any
        | TypeDesc::Int
        | TypeDesc::Float
        | TypeDesc::Bool
        | TypeDesc::Char
        | TypeDesc::Str
        | TypeDesc::Bytes
        | TypeDesc::Decimal
        | TypeDesc::BigInt
        | TypeDesc::Callable
        | TypeDesc::Class { .. }
        | TypeDesc::Enum { .. }
        | TypeDesc::Set(_) => v,
        TypeDesc::Nullable(inner) => {
            if v.is_null() {
                v
            } else {
                walk(heap, v, inner, scope, visiting)
            }
        }
        TypeDesc::Map(key, val) => {
            if let Some(entries) = object_entries(heap, v) {
                if !matches!(**key, TypeDesc::Str | TypeDesc::Any) {
                    return v;
                }
                let mut map = ValueMap::with_capacity(entries.len());
                for (name, x) in entries {
                    let x = walk(heap, x, val, scope, visiting);
                    let name = heap.alloc_str_interned(&name);
                    let key = heap.canonical_map_key(name);
                    map.insert(key, x);
                }
                return heap.alloc_map_vm(map);
            }
            let Some(map) = heap.map_of(v) else {
                return v;
            };
            let entries: Vec<_> = map.read().iter().map(|(k, x)| (*k, *x)).collect();
            for (k, x) in entries {
                let replaced = walk(heap, x, val, scope, visiting);
                if replaced != x {
                    map.write().insert(k, replaced);
                }
            }
            v
        }
        TypeDesc::Dict(val) => {
            if let Some(entries) = object_entries(heap, v) {
                for (name, x) in entries {
                    let replaced = walk(heap, x, val, scope, visiting);
                    if replaced != x
                        && let Some(HeapObj::Object(obj) | HeapObj::Record(obj)) =
                            heap.get(v.as_heap())
                    {
                        obj.borrow().set_field_str(&name, replaced);
                    }
                }
                return v;
            }
            let Some(map) = heap.map_of(v) else {
                return v;
            };
            let entries: Vec<_> = map.read().iter().map(|(k, x)| (*k, *x)).collect();
            for (k, x) in entries {
                let replaced = walk(heap, x, val, scope, visiting);
                if replaced != x {
                    map.write().insert(k, replaced);
                }
            }
            v
        }
        TypeDesc::Array(inner) => {
            let Some(items) = elements(heap, v) else {
                return v;
            };
            for (i, item) in items.into_iter().enumerate() {
                let replaced = walk(heap, item, inner, scope, visiting);
                if replaced != item {
                    store_element(heap, v, i, replaced);
                }
            }
            v
        }
        TypeDesc::Tuple(slots) => {
            let Some(items) = elements(heap, v) else {
                return v;
            };
            let is_array = matches!(heap.get(v.as_heap()), Some(HeapObj::Array(_)));
            if is_array && items.len() != slots.len() {
                return v;
            }
            let mut converted = Vec::with_capacity(items.len());
            for (i, (item, slot)) in items.into_iter().zip(slots).enumerate() {
                let replaced = walk(heap, item, slot, scope, visiting);
                if replaced != item && !is_array {
                    store_element(heap, v, i, replaced);
                }
                converted.push(replaced);
            }
            if is_array {
                heap.alloc_tuple_vm(converted)
            } else {
                v
            }
        }
        TypeDesc::Args { fixed, rest } => {
            let Some(items) = elements(heap, v) else {
                return v;
            };
            for (i, item) in items.into_iter().enumerate() {
                let Some(expected) = fixed.get(i).or(rest.as_deref()) else {
                    break;
                };
                let replaced = walk(heap, item, expected, scope, visiting);
                if replaced != item {
                    store_element(heap, v, i, replaced);
                }
            }
            v
        }
        TypeDesc::Object(fields) => {
            let Some(entries) = object_entries(heap, v) else {
                return v;
            };
            let inner = Scope {
                desc,
                parent: scope,
            };
            for (name, x) in entries {
                let Some(field) = fields.iter().find(|field| field.name == name) else {
                    continue;
                };
                let replaced = walk(heap, x, &field.ty, Some(&inner), visiting);
                if replaced != x
                    && let Some(HeapObj::Object(obj) | HeapObj::Record(obj)) = heap.get(v.as_heap())
                {
                    obj.borrow().set_field_str(&name, replaced);
                }
            }
            v
        }
        TypeDesc::Enclosing { up, .. } => {
            let Some(mut target) = scope else {
                return v;
            };
            for _ in 0..*up {
                match target.parent {
                    Some(parent) => target = parent,
                    None => return v,
                }
            }
            let key = (v.raw_payload(), target.desc as *const TypeDesc);
            if !v.is_heap() || visiting.contains(&key) {
                return v;
            }
            visiting.push(key);
            let replaced = walk(heap, v, target.desc, target.parent, visiting);
            visiting.pop();
            replaced
        }
    }
}
