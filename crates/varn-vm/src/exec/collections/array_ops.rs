use super::*;

pub(crate) fn array_length(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(val.as_heap()) {
            return Ok(VmValue::from_i32(a.len() as i32));
        }
        if let Some(HeapObj::Str(s)) = heap.get(val.as_heap()) {
            return Ok(VmValue::from_i32(s.chars().count() as i32));
        }
    }
    Err(RuntimeError::new("OpArrayLength: not an array"))
}

pub(crate) fn bytes_length(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_heap() {
        if let Some(HeapObj::Buffer(b)) = heap.get(val.as_heap()) {
            return Ok(VmValue::from_i32(b.len() as i32));
        }
    }
    Err(RuntimeError::new("OpBytesLength: not bytes"))
}

pub(crate) fn array_push(arr: VmValue, val: VmValue, heap: &mut Heap) -> VmResult<()> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(arr.as_heap()) {
            a.push_vm(val);
            heap.write_barrier(arr.as_heap(), val);
            return Ok(());
        }
    }
    Err(RuntimeError::new("OpArrayPush: not an array"))
}

pub(crate) fn array_pop(arr: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(arr.as_heap()) {
            let v = a.pop_vm().unwrap_or(VmValue::null());
            return Ok(v);
        }
    }
    Err(RuntimeError::new("OpArrayPop: not an array"))
}

pub(crate) fn array_extend(dst: VmValue, src: VmValue, heap: &Heap) -> VmResult<()> {
    if dst.is_heap() && src.is_heap() {
        if let (Some(HeapObj::Array(da)), Some(HeapObj::Array(sa))) =
            (heap.get(dst.as_heap()), heap.get(src.as_heap()))
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
                heap_mut.write_barrier(dst.as_heap(), item);
            }
            return Ok(());
        }
    }
    Err(RuntimeError::new("OpArrayExtend: not arrays"))
}
