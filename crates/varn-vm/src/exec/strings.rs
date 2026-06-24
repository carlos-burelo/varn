use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

pub fn str_length(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_sso() {
        return Ok(VmValue::from_i32(val.sso_len() as i32));
    }
    if val.is_heap() {
        if let Some(HeapObj::Str(s)) = heap.get(val.as_heap_idx()) {
            return Ok(VmValue::from_i32(s.chars().count() as i32));
        }
    }
    Err(RuntimeError::new("OpStrLength: not a string"))
}

pub fn str_concat(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    let sa = heap.str_repr(a);
    let sb = heap.str_repr(b);
    let mut out = String::with_capacity(sa.len() + sb.len());
    out.push_str(&sa);
    out.push_str(&sb);
    heap.alloc_str_dynamic(out)
}

pub fn to_string(val: VmValue, heap: &mut Heap) -> VmValue {
    let s = heap.str_repr(val);
    heap.alloc_str_dynamic(s)
}

pub fn str_slice(
    str_val: VmValue,
    start: VmValue,
    end: VmValue,
    heap: &mut Heap,
) -> VmResult<VmValue> {
    let s_owned: String = if str_val.is_sso() {
        let mut buf = [0u8; 5];
        str_val.sso_as_str(&mut buf).to_owned()
    } else if str_val.is_heap() {
        match heap.get(str_val.as_heap_idx()) {
            Some(HeapObj::Str(s)) => s.to_string(),
            _ => return Err(RuntimeError::new("OpStrSlice: not a string")),
        }
    } else {
        return Err(RuntimeError::new("OpStrSlice: not a string"));
    };

    let chars: Vec<char> = s_owned.chars().collect();
    let len = chars.len() as i32;

    let si = normalize_index(start.to_i32(), len);
    let ei = if end.is_null() {
        len as usize
    } else {
        normalize_index(end.to_i32(), len)
    };

    let si = si.min(chars.len());
    let ei = ei.min(chars.len());
    let ei = ei.max(si);

    let result: String = chars[si..ei].iter().collect();
    Ok(heap.alloc_str_dynamic(result))
}

fn normalize_index(idx: i32, len: i32) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        idx as usize
    }
}
