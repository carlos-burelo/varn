use crate::exec::advanced::{instanceof, typeof_val};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::cell::RefCell;
use varn_core::{RuntimeErrorKind, RuntimeKind, TypeDesc};
use varn_types::vm_value::ArrayRepr;
use varn_types::{NativeCtx, NativeError, NativeFnResult, VmArray};

pub(crate) enum Fit {
    Exact,
    Widened(VmValue),
}

pub(crate) fn mismatch(message: String) -> NativeError {
    NativeError::rethrow(RuntimeErrorKind::TypeError, message, None)
}

pub(crate) fn conform_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let [value, desc, witnesses @ ..] = args else {
        return Err(mismatch("conform: missing type descriptor".to_string()));
    };
    let text = ctx
        .str_owned(*desc)
        .ok_or_else(|| mismatch("conform: type descriptor is not a string".to_string()))?;
    match ctx.boundary() {
        Some(boundary) => boundary.conform(*value, &text, witnesses),
        None => Err(mismatch("conform: this context has no heap".to_string())),
    }
}

pub(crate) fn conform(
    heap: &Heap,
    value: VmValue,
    text: &str,
    witnesses: &[VmValue],
) -> NativeFnResult {
    let desc = TypeDesc::decode(text)
        .ok_or_else(|| mismatch(format!("conform: malformed type descriptor '{text}'")))?;
    match fit(heap, value, &desc, witnesses) {
        Some(Fit::Exact) => Ok(value),
        Some(Fit::Widened(widened)) => Ok(widened),
        None => Err(mismatch(format!(
            "type mismatch: expected '{desc}', got '{}'",
            typeof_val(value, heap)
        ))),
    }
}

fn exact(holds: bool) -> Option<Fit> {
    holds.then_some(Fit::Exact)
}

fn kind_is(heap: &Heap, v: VmValue, kind: RuntimeKind) -> bool {
    typeof_val(v, heap) == kind.name()
}

pub(crate) fn fit(
    heap: &Heap,
    v: VmValue,
    desc: &TypeDesc,
    witnesses: &[VmValue],
) -> Option<Fit> {
    let cx = Cx {
        heap,
        witnesses,
        visiting: RefCell::new(Vec::new()),
    };
    fit_in(&cx, v, desc, None)
}

struct Cx<'a> {
    heap: &'a Heap,
    witnesses: &'a [VmValue],
    visiting: RefCell<Vec<(u64, *const TypeDesc)>>,
}

struct Scope<'a> {
    desc: &'a TypeDesc,
    parent: Option<&'a Scope<'a>>,
}

fn fit_in(cx: &Cx, v: VmValue, desc: &TypeDesc, scope: Option<&Scope>) -> Option<Fit> {
    let heap = cx.heap;
    let witnesses = cx.witnesses;
    match desc {
        TypeDesc::Any => Some(Fit::Exact),
        TypeDesc::Int => exact(v.is_int()),
        TypeDesc::Float => {
            if v.is_f64() {
                Some(Fit::Exact)
            } else if v.is_int() {
                Some(Fit::Widened(VmValue::from_f64(v.as_int() as f64)))
            } else {
                None
            }
        }
        TypeDesc::Bool => exact(v.is_bool()),
        TypeDesc::Str => exact(heap.is_string(v)),
        TypeDesc::Char => exact(kind_is(heap, v, RuntimeKind::Char)),
        TypeDesc::Bytes => exact(kind_is(heap, v, RuntimeKind::Bytes)),
        TypeDesc::Decimal => exact(kind_is(heap, v, RuntimeKind::Decimal)),
        TypeDesc::BigInt => exact(kind_is(heap, v, RuntimeKind::BigInt)),
        TypeDesc::Callable => exact(
            kind_is(heap, v, RuntimeKind::Function) || kind_is(heap, v, RuntimeKind::Class),
        ),
        TypeDesc::Array(el) => {
            if !v.is_heap() {
                return None;
            }
            let Some(HeapObj::Array(arr)) = heap.get(v.as_heap()) else {
                return None;
            };
            exact(fit_elements(cx, arr, el, scope))
        }
        TypeDesc::Tuple(els) => {
            if !v.is_heap() {
                return None;
            }
            let Some(HeapObj::Tuple(arr)) = heap.get(v.as_heap()) else {
                return None;
            };
            if arr.len() != els.len() {
                return None;
            }
            for (i, el) in els.iter().enumerate() {
                match fit_in(cx, arr.get_vm(i)?, el, scope)? {
                    Fit::Exact => {}
                    Fit::Widened(widened) => {
                        arr.set_vm(i, widened);
                    }
                }
            }
            Some(Fit::Exact)
        }
        TypeDesc::Map(key, val) => {
            if !v.is_heap() {
                return None;
            }
            if let Some(HeapObj::Object(obj) | HeapObj::Record(obj)) = heap.get(v.as_heap()) {
                if !matches!(**key, TypeDesc::Str | TypeDesc::Any) {
                    return None;
                }
                let obj = obj.borrow();
                for slot in 0..obj.slot_count() {
                    let Some(x) = obj.field_at(slot) else {
                        continue;
                    };
                    match fit_in(cx, x, val, scope)? {
                        Fit::Exact => {}
                        Fit::Widened(widened) => {
                            obj.set_field_at(slot, widened);
                        }
                    }
                }
                return Some(Fit::Exact);
            }
            let map = heap.map_of(v)?;
            let mut widened = Vec::new();
            for (k, x) in map.read().iter() {
                match fit_in(cx, k.0, key, scope)? {
                    Fit::Exact => {}
                    Fit::Widened(_) => return None,
                }
                match fit_in(cx, *x, val, scope)? {
                    Fit::Exact => {}
                    Fit::Widened(w) => widened.push((*k, w)),
                }
            }
            for (k, w) in widened {
                map.write().insert(k, w);
            }
            Some(Fit::Exact)
        }
        TypeDesc::Set(el) => {
            let set = heap.set_of(v)?;
            for k in set.read().iter() {
                match fit_in(cx, k.0, el, scope)? {
                    Fit::Exact => {}
                    Fit::Widened(_) => return None,
                }
            }
            Some(Fit::Exact)
        }
        TypeDesc::Class { witness, .. } => {
            let class = *witnesses.get(*witness as usize)?;
            exact(class.is_null() || instanceof(v, class, heap))
        }
        TypeDesc::Enum { witness, name } => {
            if !v.is_heap() {
                return None;
            }
            let Some(HeapObj::EnumVariant(data)) = heap.get(v.as_heap()) else {
                return None;
            };
            let class = *witnesses.get(*witness as usize)?;
            if class.is_null() {
                return exact(data.enum_name.as_ref() == name.as_str());
            }
            exact(instanceof(v, class, heap))
        }
        TypeDesc::Nullable(inner) => {
            if v.is_null() {
                Some(Fit::Exact)
            } else {
                fit_in(cx, v, inner, scope)
            }
        }
        TypeDesc::Args { fixed, rest } => {
            if !v.is_heap() {
                return Some(Fit::Exact);
            }
            let Some(HeapObj::Array(arr) | HeapObj::Tuple(arr)) = heap.get(v.as_heap()) else {
                return Some(Fit::Exact);
            };
            for i in 0..arr.len() {
                let Some(expected) = fixed.get(i).or(rest.as_deref()) else {
                    break;
                };
                match fit_in(cx, arr.get_vm(i)?, expected, scope)? {
                    Fit::Exact => {}
                    Fit::Widened(widened) => {
                        arr.set_vm(i, widened);
                    }
                }
            }
            Some(Fit::Exact)
        }
        TypeDesc::Object(fields) => {
            if !v.is_heap() {
                return None;
            }
            let Some(HeapObj::Object(obj) | HeapObj::Record(obj)) = heap.get(v.as_heap()) else {
                return None;
            };
            let obj = obj.borrow();
            let inner = Scope {
                desc,
                parent: scope,
            };
            let scope = Some(&inner);
            let mut present = 0;
            for field in fields {
                let value = obj.get_field(&field.name).filter(|x| !x.is_null());
                let Some(x) = value else {
                    if obj.get_field(&field.name).is_some() {
                        present += 1;
                    }
                    if field.optional || matches!(field.ty, TypeDesc::Nullable(_) | TypeDesc::Any) {
                        continue;
                    }
                    return None;
                };
                present += 1;
                match fit_in(cx, x, &field.ty, scope)? {
                    Fit::Exact => {}
                    Fit::Widened(widened) => obj.set_field_str(&field.name, widened),
                }
            }
            exact(present == obj.shape().property_names.len())
        }
        TypeDesc::Enclosing { up, .. } => {
            let mut target = scope?;
            for _ in 0..*up {
                target = target.parent?;
            }
            let key = (v.raw_payload(), target.desc as *const TypeDesc);
            if v.is_heap() {
                if cx.visiting.borrow().contains(&key) {
                    return Some(Fit::Exact);
                }
                cx.visiting.borrow_mut().push(key);
            }
            let outcome = fit_in(cx, v, target.desc, target.parent);
            if v.is_heap() {
                cx.visiting.borrow_mut().pop();
            }
            outcome
        }
    }
}

fn fit_elements(cx: &Cx, arr: &VmArray, el: &TypeDesc, scope: Option<&Scope>) -> bool {
    let unboxed_fits = match arr.repr() {
        ArrayRepr::I64(_) => matches!(el, TypeDesc::Int),
        ArrayRepr::F64(_) => matches!(el, TypeDesc::Float),
        ArrayRepr::Boxed(_) => false,
    };
    if unboxed_fits || el.is_any() {
        return true;
    }
    for i in 0..arr.len() {
        let Some(x) = arr.get_vm(i) else {
            return false;
        };
        match fit_in(cx, x, el, scope) {
            Some(Fit::Exact) => {}
            Some(Fit::Widened(widened)) => {
                arr.set_vm(i, widened);
            }
            None => return false,
        }
    }
    true
}
