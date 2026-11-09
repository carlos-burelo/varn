use crate::exec::value_views::{elements, object_entries, object_layout, ObjectLayout};
use crate::heap::Heap;
use crate::value::VmValue;
use std::sync::Arc;
use varn_core::TypeDesc;
use varn_types::value::ValueMap;

struct Scope<'a> {
    desc: &'a TypeDesc,
    parent: Option<&'a Scope<'a>>,
}

fn rebuild(heap: &mut Heap, layout: &ObjectLayout, values: Vec<VmValue>) -> VmValue {
    if layout.shape.class.is_none() {
        heap.alloc_object_with_shape(&layout.shape, values)
    } else {
        heap.alloc_object_pairs(layout.names().iter().map(Arc::clone).zip(values))
    }
}

thread_local! {
    static LAST_ALIGNMENT: std::cell::Cell<(u32, usize, bool)> =
        const { std::cell::Cell::new((0, 0, false)) };
}

fn aligned_with(
    shape: &varn_types::Shape,
    fields: &[varn_core::type_desc::FieldDesc],
    desc: usize,
) -> bool {
    let (last_shape, last_desc, last) = LAST_ALIGNMENT.with(std::cell::Cell::get);
    if last_shape == shape.id && last_desc == desc {
        return last;
    }
    let names = shape.ordered_names();
    let aligned = names.len() == fields.len()
        && names
            .iter()
            .zip(fields)
            .all(|(name, field)| field.name == **name);
    LAST_ALIGNMENT.with(|cell| cell.set((shape.id, desc, aligned)));
    aligned
}

pub(crate) fn project(
    heap: &mut Heap,
    stack_floor: u64,
    value: VmValue,
    desc: &TypeDesc,
) -> VmValue {
    let mut visiting = Vec::new();
    walk(heap, stack_floor, value, desc, None, &mut visiting)
}

fn walk(
    heap: &mut Heap,
    stack_floor: u64,
    v: VmValue,
    desc: &TypeDesc,
    scope: Option<&Scope>,
    visiting: &mut Vec<(u64, *const TypeDesc)>,
) -> VmValue {
    if crate::frame::native_stack_low(stack_floor) {
        return v;
    }
    match desc {
        TypeDesc::Any
        | TypeDesc::Literal(_)
        | TypeDesc::Int
        | TypeDesc::Bool
        | TypeDesc::Char
        | TypeDesc::Str
        | TypeDesc::Bytes
        | TypeDesc::Decimal
        | TypeDesc::BigInt
        | TypeDesc::Callable
        | TypeDesc::Class { .. }
        | TypeDesc::Enum { .. }
        | TypeDesc::Set(_)
        | TypeDesc::Args { .. } => v,
        TypeDesc::Float if v.is_int() => VmValue::from_f64(v.as_int() as f64),
        TypeDesc::Float => v,
        TypeDesc::Nullable(inner) => {
            if v.is_null() {
                v
            } else {
                walk(heap, stack_floor, v, inner, scope, visiting)
            }
        }
        TypeDesc::Array(inner) => {
            let Some(items) = elements(heap, v) else {
                return v;
            };
            let items = items
                .into_iter()
                .map(|x| walk(heap, stack_floor, x, inner, scope, visiting))
                .collect();
            heap.alloc_array_vm(items)
        }
        TypeDesc::Tuple(slots) => {
            let Some(items) = elements(heap, v) else {
                return v;
            };
            if items.len() != slots.len() {
                return v;
            }
            let items = items
                .into_iter()
                .zip(slots)
                .map(|(x, ty)| walk(heap, stack_floor, x, ty, scope, visiting))
                .collect();
            heap.alloc_tuple_vm(items)
        }
        TypeDesc::Map(key, inner) => {
            let entries = if let Some(entries) = object_entries(heap, v) {
                if !matches!(**key, TypeDesc::Str | TypeDesc::Any) {
                    return v;
                }
                entries
                    .into_iter()
                    .map(|(name, x)| (heap.alloc_str_interned(&name), x))
                    .collect::<Vec<_>>()
            } else if let Some(map) = heap.map_of(v) {
                map.read().iter().map(|(key, x)| (key.0, *x)).collect()
            } else {
                return v;
            };
            let mut map = ValueMap::with_capacity(entries.len());
            for (key, x) in entries {
                let x = walk(heap, stack_floor, x, inner, scope, visiting);
                map.insert(heap.canonical_map_key(key), x);
            }
            heap.alloc_map_vm(map)
        }
        TypeDesc::Object(fields) => {
            let Some(layout) = object_layout(heap, v) else {
                return v;
            };
            let names = layout.names();
            let inner = Scope {
                desc,
                parent: scope,
            };
            if aligned_with(&layout.shape, fields, desc as *const TypeDesc as usize) {
                let values: Vec<VmValue> = layout
                    .values
                    .iter()
                    .zip(fields)
                    .map(|(x, field)| {
                        walk(heap, stack_floor, *x, &field.ty, Some(&inner), visiting)
                    })
                    .collect();
                return rebuild(heap, &layout, values);
            }
            if names
                .iter()
                .any(|name| !fields.iter().any(|field| field.name == **name))
            {
                return v;
            }
            let mut pairs: Vec<(Arc<str>, VmValue)> = Vec::with_capacity(fields.len());
            for field in fields {
                if let Some(at) = names.iter().position(|name| **name == *field.name) {
                    let projected = walk(
                        heap,
                        stack_floor,
                        layout.values[at],
                        &field.ty,
                        Some(&inner),
                        visiting,
                    );
                    pairs.push((Arc::clone(&names[at]), projected));
                }
            }
            heap.alloc_object_pairs(pairs)
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
            let projected = walk(heap, stack_floor, v, target.desc, target.parent, visiting);
            visiting.pop();
            projected
        }
    }
}
