use crate::exec::advanced::{instanceof, typeof_val};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::cell::RefCell;
use varn_core::{RuntimeErrorKind, RuntimeKind, TypeDesc};
use varn_types::vm_value::ArrayRepr;
use varn_types::{NativeCtx, NativeError, NativeFnResult, VmArray};

pub(crate) fn mismatch(message: String) -> NativeError {
    NativeError::rethrow(RuntimeErrorKind::TypeError, message, None)
}

fn descriptor_args(
    ctx: &mut dyn NativeCtx,
    args: &[VmValue],
) -> Result<(VmValue, String), NativeError> {
    let [value, desc, ..] = args else {
        return Err(mismatch("conform: missing type descriptor".to_string()));
    };
    let text = ctx
        .str_owned(*desc)
        .ok_or_else(|| mismatch("conform: type descriptor is not a string".to_string()))?;
    Ok((*value, text))
}

pub(crate) fn conform_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let (value, text) = descriptor_args(ctx, args)?;
    match ctx.boundary_mut() {
        Some(boundary) => boundary.conform(value, &text, &args[2..]),
        None => Err(mismatch("conform: this context has no heap".to_string())),
    }
}

pub(crate) fn decode_native(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let (value, text) = descriptor_args(ctx, args)?;
    match ctx.boundary_mut() {
        Some(boundary) => boundary.decode(value, &text, &args[2..]),
        None => Err(mismatch("decode: this context has no heap".to_string())),
    }
}

fn parse_descriptor(text: &str) -> Result<TypeDesc, NativeError> {
    TypeDesc::decode(text)
        .ok_or_else(|| mismatch(format!("conform: malformed type descriptor '{text}'")))
}

pub(crate) fn conform(
    heap: &mut Heap,
    stack_floor: u64,
    value: VmValue,
    text: &str,
    witnesses: &[VmValue],
) -> NativeFnResult {
    conform_to(
        heap,
        stack_floor,
        value,
        &parse_descriptor(text)?,
        witnesses,
    )
}

pub(crate) fn decode(
    heap: &mut Heap,
    stack_floor: u64,
    value: VmValue,
    text: &str,
    witnesses: &[VmValue],
) -> NativeFnResult {
    let desc = parse_descriptor(text)?;
    let projected = crate::exec::decode::project(heap, stack_floor, value, &desc);
    conform_to(heap, stack_floor, projected, &desc, witnesses)
}

fn conform_to(
    heap: &mut Heap,
    stack_floor: u64,
    value: VmValue,
    desc: &TypeDesc,
    witnesses: &[VmValue],
) -> NativeFnResult {
    match fit(heap, stack_floor, value, desc, witnesses) {
        Some(()) => Ok(value),
        None => Err(mismatch(format!(
            "type mismatch: expected '{desc}', got '{}'",
            typeof_val(value, heap)
        ))),
    }
}

fn exact(holds: bool) -> Option<()> {
    holds.then_some(())
}

fn kind_is(heap: &Heap, v: VmValue, kind: RuntimeKind) -> bool {
    typeof_val(v, heap) == kind.name()
}

pub(crate) fn fit(
    heap: &Heap,
    stack_floor: u64,
    v: VmValue,
    desc: &TypeDesc,
    witnesses: &[VmValue],
) -> Option<()> {
    let cx = Cx {
        stack_floor,
        heap,
        witnesses,
        visiting: RefCell::new(Vec::new()),
    };
    fit_in(&cx, v, desc, None)
}

struct Cx<'a> {
    stack_floor: u64,
    heap: &'a Heap,
    witnesses: &'a [VmValue],
    visiting: RefCell<Vec<(u64, *const TypeDesc)>>,
}

struct Scope<'a> {
    desc: &'a TypeDesc,
    parent: Option<&'a Scope<'a>>,
}

fn fit_in(cx: &Cx, v: VmValue, desc: &TypeDesc, scope: Option<&Scope>) -> Option<()> {
    if crate::frame::native_stack_low(cx.stack_floor) {
        return None;
    }
    let heap = cx.heap;
    let witnesses = cx.witnesses;
    match desc {
        TypeDesc::Any => Some(()),
        TypeDesc::Int => exact(v.is_int()),
        TypeDesc::Float => exact(v.is_f64()),
        TypeDesc::Bool => exact(v.is_bool()),
        TypeDesc::Str => exact(heap.is_string(v)),
        TypeDesc::Char => exact(kind_is(heap, v, RuntimeKind::Char)),
        TypeDesc::Bytes => exact(kind_is(heap, v, RuntimeKind::Bytes)),
        TypeDesc::Decimal => exact(kind_is(heap, v, RuntimeKind::Decimal)),
        TypeDesc::BigInt => exact(kind_is(heap, v, RuntimeKind::BigInt)),
        TypeDesc::Callable => {
            exact(kind_is(heap, v, RuntimeKind::Function) || kind_is(heap, v, RuntimeKind::Class))
        }
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
                fit_in(cx, arr.get_vm(i)?, el, scope)?;
            }
            Some(())
        }
        TypeDesc::Map(key, val) => {
            let map = heap.map_of(v)?;
            for (k, x) in map.read().iter() {
                fit_in(cx, k.0, key, scope)?;
                fit_in(cx, *x, val, scope)?;
            }
            Some(())
        }
        TypeDesc::Dict(val) => {
            if !v.is_heap() {
                return None;
            }
            if let Some(HeapObj::Object(obj)) = heap.get(v.as_heap()) {
                let obj = obj.borrow();
                for slot in 0..obj.slot_count() {
                    if let Some(x) = obj.field_at(slot) {
                        fit_in(cx, x, val, scope)?;
                    }
                }
                return Some(());
            }
            let map = heap.map_of(v)?;
            for (key, value) in map.read().iter() {
                if !heap.is_string(key.0) {
                    return None;
                }
                fit_in(cx, *value, val, scope)?;
            }
            Some(())
        }
        TypeDesc::Set(el) => {
            let set = heap.set_of(v)?;
            for k in set.read().iter() {
                fit_in(cx, k.0, el, scope)?;
            }
            Some(())
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
                return exact(data.def.enum_name.as_ref() == name.as_str());
            }
            exact(instanceof(v, class, heap))
        }
        TypeDesc::Nullable(inner) => {
            if v.is_null() {
                Some(())
            } else {
                fit_in(cx, v, inner, scope)
            }
        }
        TypeDesc::Args { fixed, rest } => {
            if !v.is_heap() {
                return Some(());
            }
            let Some(HeapObj::Array(arr) | HeapObj::Tuple(arr)) = heap.get(v.as_heap()) else {
                return Some(());
            };
            for i in 0..arr.len() {
                let Some(expected) = fixed.get(i).or(rest.as_deref()) else {
                    break;
                };
                fit_in(cx, arr.get_vm(i)?, expected, scope)?;
            }
            Some(())
        }
        TypeDesc::Object(fields) | TypeDesc::Record(fields) => {
            if !v.is_heap() {
                return None;
            }
            if matches!(desc, TypeDesc::Record(_))
                && !matches!(heap.get(v.as_heap()), Some(HeapObj::Record(_)))
            {
                return None;
            }
            if matches!(heap.get(v.as_heap()), Some(HeapObj::Record(_)))
                && matches!(desc, TypeDesc::Object(_))
                && fields.iter().any(|field| !field.readonly)
            {
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
                fit_in(cx, x, &field.ty, scope)?;
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
                    return Some(());
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
        if fit_in(cx, x, el, scope).is_none() {
            return false;
        }
    }
    true
}
