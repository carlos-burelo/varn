use crate::exec::normalize::{elements, object_entries, store_element};
use crate::heap::Heap;
use crate::value::VmValue;
use std::sync::Arc;
use varn_core::TypeDesc;

struct Scope<'a> {
    desc: &'a TypeDesc,
    parent: Option<&'a Scope<'a>>,
}

pub(crate) fn project(heap: &mut Heap, value: VmValue, desc: &TypeDesc) -> VmValue {
    let mut visiting = Vec::new();
    walk(heap, value, desc, None, &mut visiting)
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
        | TypeDesc::Set(_)
        | TypeDesc::Args { .. } => v,
        TypeDesc::Nullable(inner) => {
            if v.is_null() {
                v
            } else {
                walk(heap, v, inner, scope, visiting)
            }
        }
        TypeDesc::Array(inner) => {
            project_elements(heap, v, |_| Some(&**inner), scope, visiting);
            v
        }
        TypeDesc::Tuple(slots) => {
            project_elements(heap, v, |i| slots.get(i), scope, visiting);
            v
        }
        TypeDesc::Dict(inner) | TypeDesc::Map(_, inner) => {
            let Some(entries) = object_entries(heap, v) else {
                return v;
            };
            let pairs: Vec<(Arc<str>, VmValue)> = entries
                .into_iter()
                .map(|(name, x)| (Arc::from(name.as_str()), walk(heap, x, inner, scope, visiting)))
                .collect();
            heap.alloc_object_pairs(pairs)
        }
        TypeDesc::Object(fields) => {
            let Some(entries) = object_entries(heap, v) else {
                return v;
            };
            let inner = Scope {
                desc,
                parent: scope,
            };
            let mut pairs: Vec<(Arc<str>, VmValue)> = Vec::with_capacity(fields.len());
            for field in fields {
                if let Some((_, x)) = entries.iter().find(|(name, _)| *name == field.name) {
                    let projected = walk(heap, *x, &field.ty, Some(&inner), visiting);
                    pairs.push((Arc::from(field.name.as_str()), projected));
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
            let projected = walk(heap, v, target.desc, target.parent, visiting);
            visiting.pop();
            projected
        }
    }
}

fn project_elements<'d>(
    heap: &mut Heap,
    v: VmValue,
    expected: impl Fn(usize) -> Option<&'d TypeDesc>,
    scope: Option<&Scope>,
    visiting: &mut Vec<(u64, *const TypeDesc)>,
) {
    let Some(items) = elements(heap, v) else {
        return;
    };
    for (i, item) in items.into_iter().enumerate() {
        let Some(desc) = expected(i) else {
            continue;
        };
        let projected = walk(heap, item, desc, scope, visiting);
        if projected != item {
            store_element(heap, v, i, projected);
        }
    }
}
