use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::{value::ObjRef, Value};

/// Build an object literal from `count` frame registers using a pre-resolved
/// shape (see `FunctionProto::resolved_shape`), avoiding the per-key
/// shape-transition lookups of the generic `build_object` path. Values are
/// read in slot order, which matches the shape's key order.
#[allow(dead_code)]
pub(crate) fn build_object_with_shape(
    store: &crate::frame_store::FrameStore,
    base: usize,
    start_reg: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
) -> VmValue {
    build_with_shape(store, base, start_reg, shape, heap, true, false)
}

#[allow(dead_code)]
pub(crate) fn build_record_with_shape(
    store: &crate::frame_store::FrameStore,
    base: usize,
    start_reg: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
) -> VmValue {
    build_with_shape(store, base, start_reg, shape, heap, true, true)
}

/// `may_hold_closure` lo decide el sitio de llamada cuando puede: si el backend
/// sabe que todos los campos son valores desboxados, ninguno es una closure y el
/// barrido que cierra upvalues sobra. El intérprete no lo sabe y pasa `true`.
pub(crate) fn build_with_shape(
    store: &crate::frame_store::FrameStore,
    base: usize,
    start_reg: usize,
    shape: Rc<varn_types::Shape>,
    heap: &mut Heap,
    may_hold_closure: bool,
    is_record: bool,
) -> VmValue {
    let oref = build_shaped(store, base, start_reg, shape, heap, may_hold_closure);
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
    store: &crate::frame_store::FrameStore,
    base: usize,
    start_reg: usize,
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
            let val_nv = store.box_reg(base, start_reg + i);
            if val_nv.is_heap() {
                // Sin clonar el closure: sólo se leen sus upvalues, y `close`
                // toca el almacén, no el heap.
                if let Some(crate::heap::HeapObj::VmClosure(nc)) = heap.get(val_nv.as_heap_idx()) {
                    for uv in &nc.upvalues {
                        uv.close(store);
                    }
                }
            }
        }
    }
    if on {
        prof::record(prof::Seg::ClosureScan, t0, prof::read());
    }

    let t1 = if on { prof::read() } else { 0 };
    // El frame por clases no es contiguo: se boxea a un Vec para el slice.
    // Solo lo usa el helper JIT (vía `build_with_shape`); el intérprete va
    // por `alloc_*_with_shape_slice` con su propio boxeo.
    let vals: Vec<VmValue> = (0..count)
        .map(|i| store.box_reg(base, start_reg + i))
        .collect();
    let oref = ObjRef::with_shape_slice(shape, &vals);
    if on {
        prof::record(prof::Seg::ObjDataAlloc, t1, prof::read());
    }
    oref
}

#[inline(always)]
pub(crate) fn array_get_index(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let idx = if key.is_int() {
            key.as_int() as usize
        } else if key.is_f64() {
            key.as_f64() as usize
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
pub(crate) fn map_get_index(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        if let Some(HeapObj::Map(m)) = heap.get(heap_idx) {
            let found = heap
                .lookup_map_key(key)
                .and_then(|k| m.borrow().get(&k).copied());
            return Ok(found.unwrap_or_else(VmValue::null));
        }
    }
    get_index(obj, key, heap)
}

#[inline(always)]
pub(crate) fn map_set_index(
    obj: VmValue,
    key: VmValue,
    val: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let maybe_m = match heap.get_mut(heap_idx) {
            Some(HeapObj::Map(mref)) => {
                if std::rc::Rc::strong_count(&mref.0) > 1 {
                    let cloned = mref.borrow().clone();
                    *mref = varn_types::value::MapRef::new(cloned);
                }
                Some(mref.clone())
            }
            _ => None,
        };
        if let Some(m) = maybe_m {
            let k = heap.canonical_map_key(key);
            m.borrow_mut().insert(k, val);
            heap.write_barrier(heap_idx, val);
            return Ok(());
        }
    }
    set_index(obj, key, val, heap)
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
        let idx = if key.is_int() {
            key.as_int() as usize
        } else if key.is_f64() {
            key.as_f64() as usize
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
            let idx = if key.is_int() {
                key.as_int() as usize
            } else {
                heap.as_int(key) as usize
            };
            let val = a.get_vm(idx).unwrap_or(VmValue::null());
            return Ok(val);
        }
        if let Some(HeapObj::Buffer(b)) = heap.get(obj.as_heap_idx()) {
            let idx = if key.is_int() {
                key.as_int() as usize
            } else {
                heap.as_int(key) as usize
            };
            let slice = b.as_slice();
            let val = slice.get(idx).map(|&byte| VmValue::from_int(byte as i64)).unwrap_or(VmValue::null());
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
        Some(HeapObj::Map(m)) => {
            let found = heap
                .lookup_map_key(key)
                .and_then(|k| m.borrow().get(&k).copied());
            Ok(found.unwrap_or_else(VmValue::null))
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
        Some(HeapObj::Map(m)) => {
            let m = m.clone();
            let k = heap.canonical_map_key(key);
            if std::rc::Rc::strong_count(&m.0) > 1 {
                if let Some(HeapObj::Map(mref)) = heap.get_mut(heap_idx) {
                    let cloned = mref.borrow().clone();
                    *mref = varn_types::value::MapRef::new(cloned);
                    mref.borrow_mut().insert(k, val);
                    heap.write_barrier(heap_idx, val);
                    return Ok(());
                }
            }
            m.borrow_mut().insert(k, val);
            heap.write_barrier(heap_idx, val);
            Ok(())
        }
        Some(HeapObj::Buffer(b)) => {
            let mut slice = b.as_mut_slice();
            if idx_i < slice.len() {
                slice[idx_i] = heap.as_int(val) as u8;
            }
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
        let heap_idx = obj.as_heap_idx();
        let maybe_obj = match heap.get(heap_idx) {
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => Some(o.clone()),
            _ => None,
        };
        if let Some(o) = maybe_obj {
            let keys: Vec<Value> = o.borrow().keys().map(|k| Value::Str(k.clone())).collect();
            return Ok(heap.alloc_array(keys));
        }
        let maybe_map = match heap.get(heap_idx) {
            Some(HeapObj::Map(m)) => Some(m.clone()),
            _ => None,
        };
        if let Some(m) = maybe_map {
            let keys: Vec<VmValue> = m.borrow().keys().map(|k| k.0).collect();
            return Ok(heap.alloc_array_vm(keys));
        }
    }
    Err(RuntimeError::new("OpObjectKeys: not an object"))
}

pub(crate) fn object_rest(obj: VmValue, exclude: &[String], heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let maybe_obj = match heap.get(heap_idx) {
            Some(HeapObj::Object(o)) => Some((false, o.clone())),
            Some(HeapObj::Record(o)) => Some((true, o.clone())),
            _ => None,
        };
        if let Some((is_record, o)) = maybe_obj {
            let kept: Vec<(Rc<str>, VmValue)> = o
                .borrow()
                .iter()
                .filter(|(k, _)| !exclude.iter().any(|e| e.as_str() == k.as_ref()))
                .collect();
            let oref = ObjRef::from_pairs(kept);
            let result_obj = if is_record {
                HeapObj::Record(oref)
            } else {
                HeapObj::Object(oref)
            };
            return Ok(VmValue::from_heap_idx(heap.alloc(result_obj)));
        }
        let maybe_map = match heap.get(heap_idx) {
            Some(HeapObj::Map(m)) => Some(m.clone()),
            _ => None,
        };
        if let Some(m) = maybe_map {
            let entries: Vec<(varn_types::value::MapKey, VmValue)> =
                m.borrow().iter().map(|(k, v)| (*k, *v)).collect();
            let mut new_m = varn_types::value::ValueMap::default();
            for (k, v) in entries {
                let s = heap.str_repr(k.0);
                if !exclude.iter().any(|e| e.as_str() == s.as_str()) {
                    new_m.insert(k, v);
                }
            }
            return Ok(heap.alloc_map_vm(new_m));
        }
    }
    Err(RuntimeError::new("OpObjectRest: not an object"))
}

pub(crate) fn object_merge(target: VmValue, spread: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if !target.is_heap() {
        return Ok(target);
    }
    let target_obj = match heap.get(target.as_heap_idx()) {
        Some(HeapObj::Object(o)) => o.clone(),
        _ => return Ok(target),
    };
    // An `Instance` has no `ObjData` to iterate: its fields live in a flat
    // payload addressed by the class layout, which is also their name order.
    if spread.is_heap() {
        let spread_idx = spread.as_heap_idx();
        if let Some(HeapObj::Instance(inst)) = heap.get(spread_idx) {
            let inst = inst.clone();
            if let Some(cls) = varn_types::ClassObj::find_by_id(inst.class_id) {
                for field in &cls.get_or_compute_layout().fields {
                    if let Some(v) = inst.read_field(field) {
                        target_obj.insert(field.name.clone(), v);
                    }
                }
            }
            return Ok(target);
        }
        let maybe_map = match heap.get(spread_idx) {
            Some(HeapObj::Map(m)) => Some(m.clone()),
            _ => None,
        };
        if let Some(m) = maybe_map {
            let entries: Vec<(varn_types::value::MapKey, VmValue)> =
                m.borrow().iter().map(|(k, v)| (*k, *v)).collect();
            for (k, nv) in entries {
                let s = heap.str_repr(k.0);
                target_obj.insert(Rc::from(s.as_str()), nv);
            }
            return Ok(target);
        }
    }
    if let Value::Object(src) = heap.extract(spread) {
        let pairs: Vec<(Rc<str>, VmValue)> = src.borrow().iter().collect();
        for (k, nv) in pairs {
            target_obj.insert(k, nv);
        }
    }
    Ok(target)
}
