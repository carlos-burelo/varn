use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::{value::ObjRef, Value};

/// Build an object literal from `count` contiguous stack values using a
/// pre-resolved shape (see `FunctionProto::resolved_shape`), avoiding the
/// per-key shape-transition lookups of the generic `build_object` path.
/// Values are read in slot order, which matches the shape's key order.
pub(crate) fn build_object_with_shape(
    stack: &[VmValue],
    values_start: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
) -> VmValue {
    build_with_shape(stack, values_start, shape, heap, true, false)
}

pub(crate) fn build_record_with_shape(
    stack: &[VmValue],
    values_start: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
) -> VmValue {
    build_with_shape(stack, values_start, shape, heap, true, true)
}

/// `may_hold_closure` lo decide el sitio de llamada cuando puede: si el backend
/// sabe que todos los campos son valores desboxados, ninguno es una closure y el
/// barrido que cierra upvalues sobra. El intérprete no lo sabe y pasa `true`.
pub(crate) fn build_with_shape(
    stack: &[VmValue],
    values_start: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
    may_hold_closure: bool,
    is_record: bool,
) -> VmValue {
    let oref = build_shaped(stack, values_start, shape, heap, may_hold_closure);
    let obj = if is_record {
        HeapObj::Record(oref)
    } else {
        HeapObj::Object(oref)
    };
    alloc_timed(heap, obj)
}

/// `Heap::alloc` con el tramo anotado: mover el `HeapObj` de 48 bytes y
/// empujarlo al nursery es uno de los candidatos a explicar los ~54 ns que el
/// allocator no explica.
#[inline(always)]
fn alloc_timed(heap: &mut Heap, obj: HeapObj) -> VmValue {
    use crate::alloc_profile as prof;
    if !prof::detail() {
        return VmValue::from_heap_idx(heap.alloc(obj));
    }
    let t0 = prof::read();
    let idx = heap.alloc(obj);
    prof::record(prof::Seg::HeapPush, t0, prof::read());
    VmValue::from_heap_idx(idx)
}

/// Parte común de objeto y record: cerrar las upvalues de los valores que sean
/// closures y copiar los campos dentro del objeto.
///
/// `with_shape_slice`, no `with_shape`: la forma con `Vec` aloca un buffer,
/// lo copia en el almacenamiento inline del objeto y lo tira — una asignación
/// entera de heap por objeto construido, sin ningún fin. Es el mismo derroche
/// que el comentario de `ObjData::with_shape_slice` documenta como corregido
/// para `JSON.parse`, y estaba en el camino principal de creación de objetos.
fn build_shaped(
    stack: &[VmValue],
    values_start: usize,
    shape: Rc<varn_types::Shape>,
    heap: &Heap,
    may_hold_closure: bool,
) -> ObjRef {
    use crate::alloc_profile as prof;
    let count = shape.property_names.len();
    let on = prof::detail();

    let t0 = if on { prof::read() } else { 0 };
    if may_hold_closure {
        for i in 0..count {
            let val_nv = stack[values_start + i];
            if val_nv.is_heap() {
                // Sin clonar el closure: sólo se leen sus upvalues, y `close`
                // toca la pila, no el heap.
                if let Some(crate::heap::HeapObj::VmClosure(nc)) = heap.get(val_nv.as_heap_idx()) {
                    for uv in &nc.upvalues {
                        uv.close(stack);
                    }
                }
            }
        }
    }
    if on {
        prof::record(prof::Seg::ClosureScan, t0, prof::read());
    }

    let t1 = if on { prof::read() } else { 0 };
    let oref = ObjRef::with_shape_slice(shape, &stack[values_start..values_start + count]);
    if on {
        prof::record(prof::Seg::ObjDataAlloc, t1, prof::read());
    }
    oref
}

#[inline(always)]
pub(crate) fn array_get_index(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let idx = if heap.is_int(key) {
            heap.as_int(key) as usize
        } else {
            heap.to_f64_val(key) as usize
        };
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get(heap_idx) {
            let val = a.get_vm(idx).unwrap_or(VmValue::null());
            return Ok(val);
        }
    }
    get_index(obj, key, heap)
}

#[inline(always)]
pub(crate) fn array_set_index(
    obj: VmValue,
    key: VmValue,
    val: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let idx = if heap.is_int(key) {
            heap.as_int(key) as usize
        } else {
            heap.to_f64_val(key) as usize
        };
        if let Some(HeapObj::Tuple(_)) = heap.get(heap_idx) {
            return Err(RuntimeError::new("TypeError: Cannot mutate tuple"));
        }
        if let Some(HeapObj::Array(a)) = heap.get_mut(heap_idx) {
            let len = a.len();
            if idx < len {
                a.set_vm(idx, val);
            } else if idx == len {
                a.push_vm(val);
            } else {
                while a.len() < idx {
                    a.push_vm(VmValue::null());
                }
                a.push_vm(val);
            }
            heap.write_barrier(heap_idx, val);
            return Ok(());
        }
    }
    set_index(obj, key, val, heap)
}

pub(crate) fn get_index(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get(obj.as_heap_idx()) {
            let idx = heap.as_int(key);
            let val = a.get_vm(idx as usize).unwrap_or(VmValue::null());
            return Ok(val);
        }
    }
    if obj.is_sso() {
        let mut buf = [0u8; 5];
        let s_str = obj.sso_as_str(&mut buf);
        let idx = heap.as_int(key) as usize;
        return match s_str.chars().nth(idx) {
            Some(c) => Ok(heap.alloc_str(c.to_string())),
            None => Ok(VmValue::null()),
        };
    }
    if !obj.is_heap() {
        return Err(RuntimeError::new("OpGetIndex: not indexable"));
    }
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Object(o) | HeapObj::Record(o)) => {
            let mut buf = [0u8; 5];
            let key_str = if key.is_sso() {
                key.sso_as_str(&mut buf)
            } else if key.is_heap() {
                if let Some(HeapObj::Str(s)) = heap.get(key.as_heap_idx()) {
                    s.as_str()
                } else {
                    ""
                }
            } else {
                ""
            };
            if !key_str.is_empty() {
                Ok(o.borrow().get_field_nv(key_str).unwrap_or(VmValue::null()))
            } else {
                let key_s = heap.str_repr(key);
                Ok(o.borrow().get_field_nv(&key_s).unwrap_or(VmValue::null()))
            }
        }
        Some(HeapObj::Str(s)) => {
            let idx = heap.as_int(key);
            let c = s.chars().nth(idx as usize);
            match c {
                Some(ch) => {
                    let s = ch.to_string();
                    Ok(heap.alloc_str(s))
                }
                None => Ok(VmValue::null()),
            }
        }
        Some(HeapObj::Range(r)) => {
            let idx = heap.as_int(key);
            let (start, end, step, inclusive) = (r.start, r.end, r.step, r.inclusive);
            let diff = end - start;
            let count = if inclusive {
                (diff / step) + 1
            } else {
                (diff + step - 1) / step
            };
            if idx >= 0 && idx < count {
                let r = start + idx * step;
                Ok(heap.make_int(r))
            } else {
                Ok(VmValue::null())
            }
        }
        _ => Err(RuntimeError::new("OpGetIndex: not indexable")),
    }
}

pub(crate) fn set_index(obj: VmValue, key: VmValue, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if !obj.is_heap() {
        return Err(RuntimeError::new("OpSetIndex: not indexable"));
    }
    let heap_idx = obj.as_heap_idx();
    let idx_i = heap.as_int(key) as usize;
    match heap.get(heap_idx) {
        Some(HeapObj::Array(a)) => {
            let len = a.len();
            if idx_i < len {
                a.set_vm(idx_i, val);
            } else if idx_i == len {
                a.push_vm(val);
            } else {
                while a.len() < idx_i {
                    a.push_vm(VmValue::null());
                }
                a.push_vm(val);
            }
            heap.write_barrier(heap_idx, val);
            Ok(())
        }
        Some(HeapObj::Object(o)) => {
            let mut buf = [0u8; 5];
            let key_str = if key.is_sso() {
                key.sso_as_str(&mut buf)
            } else if key.is_heap() {
                if let Some(HeapObj::Str(s)) = heap.get(key.as_heap_idx()) {
                    s.as_str()
                } else {
                    ""
                }
            } else {
                ""
            };
            if !key_str.is_empty() {
                o.set_field_str(key_str, val);
            } else {
                let key_s = heap.str_repr(key);
                o.set_field_str(&key_s, val);
            }
            heap.write_barrier(heap_idx, val);
            Ok(())
        }
        _ => Err(RuntimeError::new("OpSetIndex: not indexable")),
    }
}

pub(crate) fn array_length(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(val.as_heap_idx()) {
            return Ok(VmValue::from_i32(a.len() as i32));
        }
        if let Some(HeapObj::Str(s)) = heap.get(val.as_heap_idx()) {
            return Ok(VmValue::from_i32(s.chars().count() as i32));
        }
    }
    Err(RuntimeError::new("OpArrayLength: not an array"))
}

pub(crate) fn array_push(arr: VmValue, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(arr.as_heap_idx()) {
            a.push_vm(val);
            heap.write_barrier(arr.as_heap_idx(), val);
            return Ok(());
        }
    }
    Err(RuntimeError::new("OpArrayPush: not an array"))
}

pub(crate) fn array_pop(arr: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(arr.as_heap_idx()) {
            let v = a.pop_vm().unwrap_or(VmValue::null());
            return Ok(v);
        }
    }
    Err(RuntimeError::new("OpArrayPop: not an array"))
}

pub(crate) fn array_extend(dst: VmValue, src: VmValue, heap: &Heap) -> VmResult<()> {
    if dst.is_heap() && src.is_heap() {
        if let (Some(HeapObj::Array(da)), Some(HeapObj::Array(sa))) =
            (heap.get(dst.as_heap_idx()), heap.get(src.as_heap_idx()))
        {
            // Snapshot source elements first (a copy, so dst == src is safe),
            // then append boxed into the destination.
            let n = sa.len();
            let mut items: Vec<VmValue> = Vec::with_capacity(n);
            for i in 0..n {
                items.push(sa.get_vm(i).unwrap_or(VmValue::null()));
            }
            for &item in &items {
                da.push_vm(item);
            }
            let heap_mut = unsafe { heap.inner_mut() };
            for &item in &items {
                heap_mut.write_barrier(dst.as_heap_idx(), item);
            }
            return Ok(());
        }
    }
    Err(RuntimeError::new("OpArrayExtend: not arrays"))
}

pub(crate) fn object_keys(obj: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Object(o)) = heap.get(obj.as_heap_idx()) {
            let keys: Vec<Value> = o
                .borrow()
                .keys()
                .map(|k| Value::Str(k.clone()))
                .collect();
            return Ok(heap.alloc_array(keys));
        }
    }
    Err(RuntimeError::new("OpObjectKeys: not an object"))
}

pub(crate) fn object_rest(obj: VmValue, exclude: &[String], heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Object(o)) = heap.get(obj.as_heap_idx()) {
            let kept: Vec<(Rc<str>, VmValue)> = o
                .borrow()
                .iter()
                .filter(|(k, _)| !exclude.iter().any(|e| e.as_str() == k.as_ref()))
                .collect();
            let oref = ObjRef::from_pairs(kept);
            return Ok(VmValue::from_heap_idx(heap.alloc(HeapObj::Object(oref))));
        }
    }
    Err(RuntimeError::new("OpObjectRest: not an object"))
}

pub(crate) fn object_merge(target: VmValue, spread: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if !target.is_heap() {
        return Ok(target);
    }
    let spread_val = heap.extract(spread);
    let target_obj = match heap.get(target.as_heap_idx()) {
        Some(HeapObj::Object(o)) => o.clone(),
        _ => return Ok(target),
    };
    if let Value::Object(src) = spread_val {
        let pairs: Vec<(Rc<str>, VmValue)> = src.borrow().iter().collect();
        for (k, nv) in pairs {
            target_obj.insert(k, nv);
        }
    }
    Ok(target)
}
