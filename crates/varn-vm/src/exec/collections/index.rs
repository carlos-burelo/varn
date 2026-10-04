use super::*;

#[inline(always)]
pub(crate) fn array_get_index(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap();
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
        let heap_idx = obj.as_heap();
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
        let heap_idx = obj.as_heap();
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
        let heap_idx = obj.as_heap();
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
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get(obj.as_heap()) {
            let idx = if key.is_int() {
                key.as_int() as usize
            } else {
                heap.as_int(key) as usize
            };
            let val = a.get_vm(idx).unwrap_or(VmValue::null());
            return Ok(val);
        }
        if let Some(HeapObj::Buffer(b)) = heap.get(obj.as_heap()) {
            let idx = if key.is_int() {
                key.as_int() as usize
            } else {
                heap.as_int(key) as usize
            };
            let slice = b.as_slice();
            let val = slice
                .get(idx)
                .map(|&byte| VmValue::from_int(byte as i64))
                .unwrap_or(VmValue::null());
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
    match heap.get(obj.as_heap()) {
        Some(HeapObj::Object(o) | HeapObj::Record(o)) => {
            let mut buf = [0u8; 5];
            let key_str = if key.is_sso() {
                key.sso_as_str(&mut buf)
            } else if key.is_heap() {
                if let Some(HeapObj::Str(s)) = heap.get(key.as_heap()) {
                    s.as_str()
                } else {
                    ""
                }
            } else {
                ""
            };
            if !key_str.is_empty() {
                Ok(o.borrow().get_field(key_str).unwrap_or(VmValue::null()))
            } else {
                let key_s = heap.str_repr(key);
                Ok(o.borrow().get_field(&key_s).unwrap_or(VmValue::null()))
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
            let r = r.clone();
            match r.nth(heap.as_int(key)) {
                Some(raw) => Ok(match r.elem {
                    varn_types::value::RangeElem::Int => VmValue::from_int(raw),
                    varn_types::value::RangeElem::Char => {
                        heap.alloc_char(varn_types::value::RangeData::char_of(raw))
                    }
                }),
                None => Ok(VmValue::null()),
            }
        }
        Some(HeapObj::Map(m)) => {
            let found = heap
                .lookup_map_key(key)
                .and_then(|k| m.borrow().get(&k).copied());
            Ok(found.unwrap_or_else(VmValue::null))
        }
        Some(
            HeapObj::Instance(_)
            | HeapObj::Class(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_),
        ) => {
            let mut buf = [0u8; 5];
            let key_str: String = if key.is_sso() {
                key.sso_as_str(&mut buf).to_string()
            } else if key.is_heap() {
                match heap.get(key.as_heap()) {
                    Some(HeapObj::Str(s)) => s.as_str().to_string(),
                    _ => heap.str_repr(key),
                }
            } else {
                heap.str_repr(key)
            };
            match crate::exec::props::get_property(obj, &key_str, heap) {
                Ok(v) if !v.is_null() => Ok(v),
                _ => match crate::exec::props::find_getter(obj, &key_str, heap) {
                    Some(g) => Ok(crate::exec::props::bind_method_to_receiver(
                        heap, obj, g, None,
                    )),
                    None => Ok(VmValue::null()),
                },
            }
        }
        _ => Err(RuntimeError::new("OpGetIndex: not indexable")),
    }
}

pub(crate) fn set_index(obj: VmValue, key: VmValue, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if !obj.is_heap() {
        return Err(RuntimeError::new("OpSetIndex: not indexable"));
    }
    let heap_idx = obj.as_heap();
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
                if let Some(HeapObj::Str(s)) = heap.get(key.as_heap()) {
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
