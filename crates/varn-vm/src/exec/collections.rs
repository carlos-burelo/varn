use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::{value::ObjRef, ObjData, RuntimeObject, Value, VmArray};

pub fn build_object_with_shape(
    stack: &mut Vec<VmValue>,
    keys: &[Rc<str>],
    heap: &mut Heap,
) -> VmValue {
    let count = keys.len();
    let mut inner = RuntimeObject::new();
    let len = stack.len();
    let values_start = len.saturating_sub(count);

    for (i, key) in keys.iter().enumerate() {
        let val_nv = stack[values_start + i];

        if val_nv.is_heap() {
            if let Some(crate::heap::HeapObj::VmClosure(nc)) = heap.get(val_nv.as_heap_idx()) {
                let nc = nc.clone();
                for uv in &nc.upvalues {
                    uv.close(stack);
                }
            }
        }

        inner.insert(key.clone(), val_nv);
    }

    stack.truncate(values_start);
    let oref = ObjRef::new(ObjData::from_inner(inner));
    VmValue::from_heap_idx(heap.alloc(HeapObj::Object(oref)))
}

pub fn build_array(stack: &mut Vec<VmValue>, count: usize, heap: &mut Heap) -> VmValue {
    let len = stack.len();
    let start = len.saturating_sub(count);
    let items: Vec<VmValue> = stack.drain(start..).collect();
    let va = VmArray::new(items);
    VmValue::from_heap_idx(heap.alloc(HeapObj::Array(va)))
}

pub fn build_object(stack: &mut Vec<VmValue>, count: usize, heap: &mut Heap) -> VmValue {
    let mut obj = RuntimeObject::new();
    let len = stack.len();
    let pairs_start = len.saturating_sub(count * 2);

    for i in (pairs_start..len).step_by(2) {
        let val_nv = stack[i + 1];
        if val_nv.is_heap() {
            if let Some(crate::heap::HeapObj::VmClosure(nc)) = heap.get(val_nv.as_heap_idx()) {
                let nc = nc.clone();
                for uv in &nc.upvalues {
                    uv.close(stack);
                }
            }
        }
    }

    let pairs: Vec<VmValue> = stack.drain(pairs_start..).collect();
    for chunk in pairs.chunks_exact(2) {
        let key_nv = chunk[0];
        let val_nv = chunk[1];
        let key = heap.str_repr(key_nv);

        obj.insert(Rc::from(key.as_str()), val_nv);
    }
    let oref = ObjRef::new(ObjData::from_inner(obj));
    VmValue::from_heap_idx(heap.alloc(HeapObj::Object(oref)))
}

pub fn get_index(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_sso() {
        let mut buf = [0u8; 5];
        let s_str = obj.sso_as_str(&mut buf);
        let idx = key.to_i32() as usize;
        return match s_str.chars().nth(idx) {
            Some(c) => Ok(heap.alloc_str(c.to_string())),
            None => Ok(VmValue::null()),
        };
    }
    if !obj.is_heap() {
        return Err(RuntimeError::new("OpGetIndex: not indexable"));
    }
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Array(a)) => {
            let idx = key.to_i32();
            let val = {
                let g = a.borrow();
                g.get(idx as usize).cloned().unwrap_or(VmValue::null())
            };
            Ok(val)
        }
        Some(HeapObj::Object(o)) => {
            let key_s = heap.str_repr(key);
            Ok(o.borrow().get_field_nv(&key_s).unwrap_or(VmValue::null()))
        }
        Some(HeapObj::Str(s)) => {
            let idx = key.to_i32();
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
            let idx = key.to_i32() as i64;
            let diff = r.end - r.start;
            let count = if r.inclusive {
                (diff / r.step) + 1
            } else {
                (diff + r.step - 1) / r.step
            };
            if idx >= 0 && idx < count {
                Ok(VmValue::from_i32((r.start + idx * r.step) as i32))
            } else {
                Ok(VmValue::null())
            }
        }
        _ => Err(RuntimeError::new("OpGetIndex: not indexable")),
    }
}

pub fn set_index(obj: VmValue, key: VmValue, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if !obj.is_heap() {
        return Err(RuntimeError::new("OpSetIndex: not indexable"));
    }
    let key_s = heap.str_repr(key);
    let idx_i = key.to_i32() as usize;
    match heap.get(obj.as_heap_idx()) {
        Some(HeapObj::Array(a)) => {
            let a = a.clone();
            let g = a.borrow_mut();
            if idx_i < g.len() {
                g[idx_i] = val;
            } else if idx_i == g.len() {
                g.push(val);
            } else {
                while g.len() < idx_i {
                    g.push(VmValue::null());
                }
                g.push(val);
            }
            heap.write_barrier(obj.as_heap_idx(), val);
            Ok(())
        }
        Some(HeapObj::Object(o)) => {
            let o = o.clone();
            o.borrow_mut().set_field_nv(Rc::from(key_s.as_str()), val);
            heap.write_barrier(obj.as_heap_idx(), val);
            Ok(())
        }
        _ => Err(RuntimeError::new("OpSetIndex: not indexable")),
    }
}

pub fn array_length(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(val.as_heap_idx()) {
            return Ok(VmValue::from_i32(a.borrow().len() as i32));
        }
        if let Some(HeapObj::Str(s)) = heap.get(val.as_heap_idx()) {
            return Ok(VmValue::from_i32(s.chars().count() as i32));
        }
    }
    Err(RuntimeError::new("OpArrayLength: not an array"))
}

pub fn array_push(arr: VmValue, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(arr.as_heap_idx()) {
            a.borrow_mut().push(val);
            heap.write_barrier(arr.as_heap_idx(), val);
            return Ok(());
        }
    }
    Err(RuntimeError::new("OpArrayPush: not an array"))
}

pub fn array_pop(arr: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(arr.as_heap_idx()) {
            let v = a.borrow_mut().pop().unwrap_or(VmValue::null());
            return Ok(v);
        }
    }
    Err(RuntimeError::new("OpArrayPop: not an array"))
}

pub fn array_extend(dst: VmValue, src: VmValue, heap: &Heap) -> VmResult<()> {
    if dst.is_heap() && src.is_heap() {
        if let (Some(HeapObj::Array(da)), Some(HeapObj::Array(sa))) =
            (heap.get(dst.as_heap_idx()), heap.get(src.as_heap_idx()))
        {
            let items: Vec<VmValue> = sa.borrow().clone();
            da.borrow_mut().extend(items.clone());
            let  heap_mut = unsafe { heap.inner_mut() };
            for &item in &items {
                heap_mut.write_barrier(dst.as_heap_idx(), item);
            }
            return Ok(());
        }
    }
    Err(RuntimeError::new("OpArrayExtend: not arrays"))
}

pub fn object_keys(obj: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Object(o)) = heap.get(obj.as_heap_idx()) {
            let keys: Vec<Value> = o
                .borrow()
                .inner
                .keys()
                .map(|k| Value::Str(k.clone().into()))
                .collect();
            return Ok(heap.alloc_array(keys));
        }
    }
    Err(RuntimeError::new("OpObjectKeys: not an object"))
}

pub fn object_rest(obj: VmValue, exclude: &[String], heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Object(o)) = heap.get(obj.as_heap_idx()) {
            let mut new_inner = RuntimeObject::new();
            for (k, v) in o.borrow().inner.iter() {
                if !exclude.iter().any(|e| e.as_str() == k.as_ref()) {
                    new_inner.insert(k.clone(), v.clone());
                }
            }
            let oref = ObjRef::new(ObjData::from_inner(new_inner));
            return Ok(VmValue::from_heap_idx(heap.alloc(HeapObj::Object(oref))));
        }
    }
    Err(RuntimeError::new("OpObjectRest: not an object"))
}

pub fn object_merge(target: VmValue, spread: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if !target.is_heap() {
        return Ok(target);
    }
    let spread_val = heap.extract(spread);
    let target_obj = match heap.get(target.as_heap_idx()) {
        Some(HeapObj::Object(o)) => o.clone(),
        _ => return Ok(target),
    };
    match spread_val {
        Value::Object(src) => {
            let src_guard = src.borrow();
            let pairs: Vec<(Rc<str>, VmValue)> = src_guard
                .inner
                .iter()
                .map(|(k, nv)| (k.clone(), nv))
                .collect();
            drop(src_guard);
            let mut dst = target_obj.borrow_mut();
            for (k, nv) in pairs {
                dst.inner.insert(k, nv);
            }
        }
        _ => {}
    }
    Ok(target)
}
