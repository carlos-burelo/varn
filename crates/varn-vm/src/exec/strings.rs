use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj, HeapStr};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::str_util::{char_len, char_range_to_bytes};


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
            let n = if s.is_ascii_cached() {
                s.len()
            } else {
                s.as_str().chars().count()
            };
            return Ok(VmValue::from_i32(n as i32));
        }
    }
    Err(RuntimeError::new("OpStrLength: not a string"))
}

/// `.length` for the property fast paths: str (char count) and Array
/// (element count), matching the native getters in varn-builtins. `None`
/// for any other receiver, which then takes the generic getter path.
pub fn fast_length(val: VmValue, heap: &Heap) -> Option<VmValue> {
    if val.is_sso() {
        return Some(VmValue::from_i32(val.sso_len() as i32));
    }
    if val.is_heap() {
        match heap.get(val.as_heap_idx()) {
            Some(HeapObj::Str(s)) => {
                let n = if s.is_ascii_cached() {
                    s.len()
                } else {
                    s.as_str().chars().count()
                };
                return Some(VmValue::from_i32(n as i32));
            }
            Some(HeapObj::Array(arr)) => {
                return Some(VmValue::from_int(arr.len() as i64));
            }
            _ => {}
        }
    }
    None
}

pub fn str_concat(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    // Accumulation fast path: `a` is the tip view of an extensible buffer.
    // Appending never disturbs shorter views of the same buffer, so this is
    // safe regardless of aliasing; the result is a longer view, O(1) amortized.
    if a.is_heap() {
        if let Some(HeapObj::Str(hs)) = heap.get(a.as_heap_idx()) {
            if let HeapStr::Ext { buf, len, ascii } = hs {
                if hs.is_tip() {
                    let buf = Rc::clone(buf);
                    let len = *len;
                    let a_ascii = ascii.get();
                    // Own `b` first: it may be a view of the same buffer, and
                    // push_str may reallocate it.
                    let sb = heap.str_repr(b);
                    // Carry the ASCII cache forward: the new view is the old
                    // prefix plus `sb`, so its state derives from both.
                    let flag = match a_ascii {
                        crate::heap::ascii_flag::NO => crate::heap::ascii_flag::NO,
                        crate::heap::ascii_flag::YES if sb.is_ascii() => {
                            crate::heap::ascii_flag::YES
                        }
                        crate::heap::ascii_flag::YES => crate::heap::ascii_flag::NO,
                        _ => crate::heap::ascii_flag::UNKNOWN,
                    };
                    unsafe { (*buf.get()).push_str(&sb) };
                    return heap.alloc_str_view(HeapStr::ext(buf, len + sb.len(), flag));
                }
            }
        }
    }

    // Render both operands straight into one output buffer: strings borrow,
    // scalars format in place — no per-operand String allocation.
    let mut out = String::with_capacity(24);
    heap.str_repr_into(a, &mut out);
    let a_len = out.len();
    heap.str_repr_into(b, &mut out);
    let total = out.len();
    if a_len >= EXT_SEED_LEN {
        out.reserve(total);
        let flag = if out.is_ascii() {
            crate::heap::ascii_flag::YES
        } else {
            crate::heap::ascii_flag::NO
        };
        return heap.alloc_str_view(HeapStr::ext(
            Rc::new(std::cell::UnsafeCell::new(out)),
            total,
            flag,
        ));
    }
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
    // Own a cheap handle (Rc clone) so the heap borrow ends before allocating.
    let mut sso_buf = [0u8; 5];
    let (hs, s): (Option<crate::heap::HeapStr>, &str) = if str_val.is_sso() {
        (None, str_val.sso_as_str(&mut sso_buf))
    } else if str_val.is_heap() {
        match heap.get(str_val.as_heap_idx()) {
            Some(HeapObj::Str(s)) => (Some(s.clone()), ""),
            _ => return Err(RuntimeError::new("OpStrSlice: not a string")),
        }
    } else {
        return Err(RuntimeError::new("OpStrSlice: not a string"));
    };
    let ascii = match &hs {
        Some(h) => h.is_ascii_cached(),
        None => true, // SSO is ASCII by construction
    };
    let s = match &hs {
        Some(h) => h.as_str(),
        None => s,
    };

    let len = char_len(s, ascii) as i32;
    let si = normalize_index(heap.as_int(start) as i32, len).min(len as usize);
    let ei = if end.is_null() {
        len as usize
    } else {
        normalize_index(heap.as_int(end) as i32, len).min(len as usize)
    };
    let ei = ei.max(si);

    let (bs, be) = char_range_to_bytes(s, ascii, si, ei);
    if bs == 0 && be == s.len() {
        return Ok(str_val);
    }
    match &hs {
        Some(h) => Ok(heap.alloc_substring(h, bs, be)),
        None => Ok(heap.alloc_str_dynamic(&s[bs..be])),
    }
}

fn normalize_index(idx: i32, len: i32) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        idx as usize
    }
}
