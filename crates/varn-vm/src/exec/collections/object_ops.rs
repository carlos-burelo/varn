use super::*;

pub(crate) fn object_keys(obj: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap();
        let maybe_obj = match heap.get(heap_idx) {
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => Some(*o),
            _ => None,
        };
        if let Some(o) = maybe_obj {
            let keys: Vec<VmValue> = o
                .borrow()
                .keys()
                .map(|k| heap.alloc_str_interned(&k))
                .collect();
            return Ok(heap.alloc_array_vm(keys));
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
        let heap_idx = obj.as_heap();
        let maybe_obj = match heap.get(heap_idx) {
            Some(HeapObj::Object(o)) => Some((false, *o)),
            Some(HeapObj::Record(o)) => Some((true, *o)),
            _ => None,
        };
        if let Some((is_record, o)) = maybe_obj {
            let kept: Vec<(Arc<str>, VmValue)> = o
                .borrow()
                .iter()
                .filter(|(k, _)| !exclude.iter().any(|e| e.as_str() == k.as_ref()))
                .collect();
            let (shape, values) = varn_types::value::ObjData::pairs_layout(kept);
            return Ok(heap.alloc_object_cell(is_record, shape, values.len(), &values));
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
    let target_obj = match heap.get(target.as_heap()) {
        Some(HeapObj::Object(o)) => *o,
        _ => return Ok(target),
    };
    // An `Instance` has no `ObjData` to iterate: its fields live in a flat
    // payload addressed by the class layout, which is also their name order.
    if spread.is_heap() {
        let spread_idx = spread.as_heap();
        if let Some(HeapObj::Instance(inst)) = heap.get(spread_idx) {
            let inst = *inst;
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
                target_obj.insert(Arc::from(s.as_str()), nv);
            }
            return Ok(target);
        }
    }
    if spread.is_heap() {
        if let Some(HeapObj::Object(src) | HeapObj::Record(src)) = heap.get(spread.as_heap()) {
            let pairs: Vec<(Arc<str>, VmValue)> = src.borrow().iter().collect();
            for (k, nv) in pairs {
                target_obj.insert(k, nv);
            }
        }
    }
    Ok(target)
}
