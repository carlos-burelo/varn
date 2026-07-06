use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj, HeapStr};
use crate::value::VmValue;
use std::rc::Rc;


/// Left operands at or above this length seed an extensible buffer, so a
/// `s = s + x` accumulation appends in place from then on. Shorter results
/// stay `Shared` — one-off concats (e.g. map keys) shouldn't pay the
/// copy-on-materialize of a buffer view.
const EXT_SEED_LEN: usize = 16;

pub fn str_length(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_sso() {
        return Ok(VmValue::from_i32(val.sso_len() as i32));
    }
    if val.is_heap() {
        if let Some(HeapObj::Str(s)) = heap.get(val.as_heap_idx()) {
            return Ok(VmValue::from_i32(s.as_str().chars().count() as i32));
        }
    }
    Err(RuntimeError::new("OpStrLength: not a string"))
}

pub fn str_concat(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    // Accumulation fast path: `a` is the tip view of an extensible buffer.
    // Appending never disturbs shorter views of the same buffer, so this is
    // safe regardless of aliasing; the result is a longer view, O(1) amortized.
    if a.is_heap() {
        if let Some(HeapObj::Str(hs)) = heap.get(a.as_heap_idx()) {
            if let HeapStr::Ext { buf, len } = hs {
                if hs.is_tip() {
                    let buf = Rc::clone(buf);
                    let len = *len;
                    // Own `b` first: it may be a view of the same buffer, and
                    // push_str may reallocate it.
                    let sb = heap.str_repr(b);
                    unsafe { (*buf.get()).push_str(&sb) };
                    return heap.alloc_str_view(HeapStr::Ext {
                        buf,
                        len: len + sb.len(),
                    });
                }
            }
        }
    }

    let sa = heap.str_repr_borrowed(a).into_owned();
    let sb = heap.str_repr(b);
    let total = sa.len() + sb.len();
    if sa.len() >= EXT_SEED_LEN {
        let mut buf = String::with_capacity(total * 2);
        buf.push_str(&sa);
        buf.push_str(&sb);
        return heap.alloc_str_view(HeapStr::Ext {
            buf: Rc::new(std::cell::UnsafeCell::new(buf)),
            len: total,
        });
    }
    let mut out = String::with_capacity(total);
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

    let si = normalize_index(heap.as_int(start) as i32, len);
    let ei = if end.is_null() {
        len as usize
    } else {
        normalize_index(heap.as_int(end) as i32, len)
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
