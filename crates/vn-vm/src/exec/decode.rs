use crate::exec::value_views::{elements, object_entries, object_layout, ObjectLayout};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::sync::Arc;
use vn_core::TypeDesc;
use vn_types::value::ValueMap;

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

pub(crate) fn aligned_with(
    shape: &vn_types::Shape,
    fields: &[vn_core::type_desc::FieldDesc],
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

fn inner_changes(desc: &TypeDesc) -> bool {
    match desc {
        TypeDesc::Float | TypeDesc::Map(..) | TypeDesc::Object(_) | TypeDesc::Enclosing { .. } => {
            true
        }
        TypeDesc::Nullable(inner) | TypeDesc::Array(inner) => inner_changes(inner),
        TypeDesc::Tuple(slots) => slots.iter().any(inner_changes),
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
        | TypeDesc::Args { .. } => false,
    }
}

fn walk_needed(desc: &TypeDesc) -> bool {
    match desc {
        TypeDesc::Array(_) | TypeDesc::Tuple(_) => true,
        TypeDesc::Nullable(inner) => walk_needed(inner),
        _ => inner_changes(desc),
    }
}

fn changed_items<'d>(
    heap: &mut Heap,
    stack_floor: u64,
    items: &[VmValue],
    keeps_kind: bool,
    desc_of: impl Fn(usize) -> &'d TypeDesc,
    scope: Option<&Scope>,
    visiting: &mut Vec<(u64, *const TypeDesc)>,
) -> Option<Vec<VmValue>> {
    let mut changed: Option<Vec<VmValue>> = (!keeps_kind).then(|| Vec::with_capacity(items.len()));
    for (at, item) in items.iter().enumerate() {
        let projected = walk(heap, stack_floor, *item, desc_of(at), scope, visiting);
        match &mut changed {
            Some(out) => out.push(projected),
            None if !projected.bits_eq(*item) => {
                let mut out = Vec::with_capacity(items.len());
                out.extend_from_slice(&items[..at]);
                out.push(projected);
                changed = Some(out);
            }
            None => {}
        }
    }
    changed
}

fn project_aligned_fields(
    heap: &mut Heap,
    stack_floor: u64,
    v: VmValue,
    desc: &TypeDesc,
    fields: &[vn_core::type_desc::FieldDesc],
    scope: Option<&Scope>,
    visiting: &mut Vec<(u64, *const TypeDesc)>,
) -> Option<bool> {
    if !v.is_heap() {
        return None;
    }
    let Some(crate::heap::HeapObj::Object(obj)) = heap.get(v.as_heap()) else {
        return None;
    };
    {
        let obj = obj.borrow();
        if !aligned_with(obj.shape(), fields, desc as *const TypeDesc as usize) {
            return None;
        }
    }
    let inner = Scope {
        desc,
        parent: scope,
    };
    for (slot, field) in fields.iter().enumerate() {
        if !inner_changes(&field.ty) {
            continue;
        }
        let x = match heap.get(v.as_heap()) {
            Some(crate::heap::HeapObj::Object(obj)) => obj.borrow().field_at(slot)?,
            _ => return None,
        };
        let projected = walk(heap, stack_floor, x, &field.ty, Some(&inner), visiting);
        if !projected.bits_eq(x) {
            return Some(false);
        }
    }
    Some(true)
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
    if crate::frame::native_stack_low(stack_floor) || !walk_needed(desc) {
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
            let keeps_kind = v.is_heap() && matches!(heap.get(v.as_heap()), Some(HeapObj::Array(_)));
            if keeps_kind && !inner_changes(inner) {
                return v;
            }
            let Some(items) = elements(heap, v) else {
                return v;
            };
            match changed_items(
                heap,
                stack_floor,
                &items,
                keeps_kind,
                |_| inner,
                scope,
                visiting,
            ) {
                Some(items) => heap.alloc_array_vm(items),
                None => v,
            }
        }
        TypeDesc::Tuple(slots) => {
            let Some(items) = elements(heap, v) else {
                return v;
            };
            if items.len() != slots.len() {
                return v;
            }
            let keeps_kind = matches!(heap.get(v.as_heap()), Some(HeapObj::Tuple(_)));
            if keeps_kind && !slots.iter().any(inner_changes) {
                return v;
            }
            match changed_items(
                heap,
                stack_floor,
                &items,
                keeps_kind,
                |at| &slots[at],
                scope,
                visiting,
            ) {
                Some(items) => heap.alloc_tuple_vm(items),
                None => v,
            }
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
            if let Some(unchanged) =
                project_aligned_fields(heap, stack_floor, v, desc, fields, scope, visiting)
                && unchanged
            {
                return v;
            }
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
