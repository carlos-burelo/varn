use super::*;

#[inline(always)]
fn element_index(key: VmValue, heap: &Heap) -> i64 {
    if key.is_int() {
        key.as_int()
    } else if key.is_f64() {
        key.as_f64() as i64
    } else {
        heap.to_f64_val(key) as i64
    }
}

#[inline(always)]
pub(crate) fn element_at(items: &varn_types::VmArray, index: i64) -> VmResult<VmValue> {
    usize::try_from(index)
        .ok()
        .and_then(|at| items.get_vm(at))
        .ok_or_else(|| RuntimeError::out_of_range(index, items.len()))
}

#[inline(always)]
pub(crate) fn store_element(items: &varn_types::VmArray, index: i64, val: VmValue) -> VmResult<()> {
    match usize::try_from(index) {
        Ok(at) if at < items.len() => {
            items.set_vm(at, val);
            Ok(())
        }
        Ok(_) | Err(_) => Err(RuntimeError::out_of_range(index, items.len())),
    }
}

pub(crate) fn char_index(heap: &Heap, text: VmValue, index: i64) -> VmResult<VmValue> {
    let found = heap.char_at(text, index);
    if found.is_null() {
        return Err(RuntimeError::out_of_range(
            index,
            heap.str_repr(text).chars().count(),
        ));
    }
    Ok(found)
}

#[inline(always)]
pub(crate) fn array_get_index(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap();
        let idx = element_index(key, heap);
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get(heap_idx) {
            return element_at(a, idx);
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
                .and_then(|k| m.read().get(&k).copied());
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
            Some(HeapObj::Map(mref)) => Some(*mref),
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Record(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Set(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Range(_)
                | HeapObj::Symbol(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => None,
        };
        if let Some(m) = maybe_m {
            let k = heap.canonical_map_key(key);
            m.write().insert(k, val);
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
        let idx = element_index(key, heap);
        if let Some(HeapObj::Tuple(_)) = heap.get(heap_idx) {
            return Err(RuntimeError::new("TypeError: Cannot mutate tuple"));
        }
        if let Some(HeapObj::Array(a)) = heap.get_mut(heap_idx) {
            store_element(a, idx, val)?;
            heap.write_barrier(heap_idx, val);
            return Ok(());
        }
    }
    set_index(obj, key, val, heap)
}

pub(crate) fn get_index(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get(obj.as_heap()) {
            return element_at(a, heap.as_int(key));
        }
        if let Some(HeapObj::Buffer(b)) = heap.get(obj.as_heap()) {
            let idx = heap.as_int(key);
            let slice = b.as_slice();
            return usize::try_from(idx)
                .ok()
                .and_then(|at| slice.get(at))
                .map(|&byte| VmValue::from_int(byte as i64))
                .ok_or_else(|| RuntimeError::out_of_range(idx, slice.len()));
        }
    }
    if obj.is_sso() {
        return char_index(heap, obj, heap.as_int(key));
    }
    if !obj.is_heap() {
        return Err(RuntimeError::new("OpGetIndex: not indexable"));
    }
    if heap.instance(obj.as_heap()).is_some() {
        return index_instance_fallback(obj, key, heap);
    }
    match heap.get(obj.as_heap()) {
        Some(HeapObj::Object(o) | HeapObj::Record(o)) => {
            let mut buf = [0u8; varn_types::vm_value::SSO_MAX_LEN];
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
        Some(HeapObj::Str(_)) => char_index(heap, obj, heap.as_int(key)),
        Some(HeapObj::Range(r)) => {
            let r = r.clone();
            let at = heap.as_int(key);
            match r.nth(at) {
                Some(raw) => Ok(match r.elem {
                    varn_types::value::RangeElem::Int => VmValue::from_int(raw),
                    varn_types::value::RangeElem::Char => {
                        heap.alloc_char(varn_types::value::RangeData::char_of(raw))
                    }
                }),
                None => Err(RuntimeError::out_of_range(at, r.len() as usize)),
            }
        }
        Some(HeapObj::Map(m)) => {
            let found = heap
                .lookup_map_key(key)
                .and_then(|k| m.read().get(&k).copied());
            Ok(found.unwrap_or_else(VmValue::null))
        }
        Some(HeapObj::Class(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_)) => {
            index_instance_fallback(obj, key, heap)
        }
        Some(
            HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Buffer(_)
            | HeapObj::VmClosure(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => Err(RuntimeError::new("OpGetIndex: not indexable")),
    }
}

fn index_instance_fallback(obj: VmValue, key: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    let mut buf = [0u8; varn_types::vm_value::SSO_MAX_LEN];
    let key_str: String = if key.is_sso() {
        key.sso_as_str(&mut buf).to_string()
    } else if key.is_heap() {
        match heap.get(key.as_heap()) {
            Some(HeapObj::Str(s)) => s.as_str().to_string(),
            Some(
                HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Record(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Range(_)
                | HeapObj::Symbol(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => heap.str_repr(key),
        }
    } else {
        heap.str_repr(key)
    };
    match crate::exec::props::get_property(obj, &key_str, heap) {
        Ok(v) if !v.is_null() => Ok(v),
        Ok(_) | Err(_) => match crate::exec::props::find_getter(obj, &key_str, heap) {
            Some(g) => Ok(crate::exec::props::bind_method_to_receiver(
                heap, obj, g, None,
            )),
            None => Ok(VmValue::null()),
        },
    }
}

pub(crate) fn set_index(obj: VmValue, key: VmValue, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if !obj.is_heap() {
        return Err(RuntimeError::new("OpSetIndex: not indexable"));
    }
    let heap_idx = obj.as_heap();
    let idx_i = heap.as_int(key);
    match heap.get(heap_idx) {
        Some(HeapObj::Array(a)) => {
            store_element(a, idx_i, val)?;
            heap.write_barrier(heap_idx, val);
            Ok(())
        }
        Some(HeapObj::Object(o)) => {
            let mut buf = [0u8; varn_types::vm_value::SSO_MAX_LEN];
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
            let m = *m;
            let k = heap.canonical_map_key(key);
            m.write().insert(k, val);
            heap.write_barrier(heap_idx, val);
            Ok(())
        }
        Some(HeapObj::Buffer(b)) => {
            let mut slice = b.as_mut_slice();
            let len = slice.len();
            let slot = usize::try_from(idx_i)
                .ok()
                .and_then(|at| slice.get_mut(at))
                .ok_or_else(|| RuntimeError::out_of_range(idx_i, len))?;
            *slot = heap.as_int(val) as u8;
            Ok(())
        }
        Some(
            HeapObj::Str(_)
            | HeapObj::Tuple(_)
            | HeapObj::Record(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::VmClosure(_)
            | HeapObj::Class(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => Err(RuntimeError::new("OpSetIndex: not indexable")),
    }
}
