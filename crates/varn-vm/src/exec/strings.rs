use crate::heap::{Heap, HeapObj, HeapStr};
use crate::strbuf::StrBuf;
use crate::value::VmValue;
use std::rc::Rc;

/// Left operands at or above this length seed an extensible buffer, so a
/// `s = s + x` accumulation appends in place from then on. Shorter results
/// stay `Shared` — one-off concats (e.g. map keys) shouldn't pay the
/// copy-on-materialize of a buffer view.

/// `.length` for the property fast paths: str (char count) and Array
/// (element count), matching the native getters in varn-builtins. `None`
/// for any other receiver, which then takes the generic getter path.
pub(crate) fn fast_length(val: VmValue, heap: &Heap) -> Option<VmValue> {
    if val.is_sso() {
        return Some(VmValue::from_i32(val.sso_len() as i32));
    }
    if val.is_heap() {
        match heap.get(val.as_heap_idx()) {
            Some(HeapObj::Str(s)) => {
                return Some(VmValue::from_i32(s.char_len() as i32));
            }
            Some(HeapObj::Array(arr)) => {
                return Some(VmValue::from_int(arr.len() as i64));
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn str_concat(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    // Fast path: two SSO strings whose combined length fits in SSO (<= 5 bytes).
    // Assembles the bytes in CPU registers with 0 allocations, 0 memory access.
    if a.is_sso() && b.is_sso() {
        let la = a.sso_len();
        let lb = b.sso_len();
        let total = la + lb;
        if total <= varn_types::vm_value::SSO_MAX_LEN {
            let packed = a.raw_payload() | (b.raw_payload() << (la * 8));
            return VmValue::from_sso_raw(total, packed);
        }
    }

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
                    let target_len = len + sb.len();
                    let cur_cap = unsafe { (&*buf.get()).capacity() };
                    if target_len > cur_cap {
                        let new_cap = (cur_cap * 2).max(target_len).max(64);
                        unsafe { (*buf.get()).reserve(new_cap - len) };
                    }
                    unsafe { (*buf.get()).push_str(&sb) };
                    return heap.alloc_str_view(HeapStr::ext(buf, target_len, flag));
                }
            }
        }
    }

    // The `"prefix" + <int>` shape, built once instead of staged through a
    // `StrBuf` and a zeroed `[u8; INLINE_STR_CAP]`. Declines to anything it
    // cannot serve, including an `Ext` left operand — but the accumulation
    // path above has already claimed those.
    if let Some(v) = heap.alloc_str_concat_inline(a, b) {
        return v;
    }

    let mut out = StrBuf::new();
    heap.str_repr_into(a, &mut out);
    heap.str_repr_into(b, &mut out);
    let s = out.into_string();
    let len = s.len();
    let flag = if s.is_ascii() {
        crate::heap::ascii_flag::YES
    } else {
        crate::heap::ascii_flag::NO
    };
    let mut st = String::with_capacity((len * 2).max(64));
    st.push_str(&s);
    let buf = Rc::new(std::cell::UnsafeCell::new(st));
    heap.alloc_str_view(HeapStr::ext(buf, len, flag))
}

pub(crate) fn to_string(val: VmValue, heap: &mut Heap) -> VmValue {
    if val.is_sso() {
        return val;
    }
    if val.is_int() {
        use crate::strbuf::{itoa, INT_MAX_DIGITS};
        let mut buf = [0u8; INT_MAX_DIGITS];
        let s = itoa(val.as_int(), &mut buf);
        if let Some(sso) = VmValue::try_from_sso(s) {
            return sso;
        }
        return heap.alloc_str_dynamic(s);
    }
    if val.is_bool() {
        return if val.as_bool() {
            VmValue::try_from_sso("true").unwrap()
        } else {
            VmValue::try_from_sso("false").unwrap()
        };
    }
    if val.is_null() {
        return VmValue::try_from_sso("null").unwrap();
    }
    if val.is_heap() {
        if let Some(HeapObj::Str(_)) = heap.get(val.as_heap_idx()) {
            return val;
        }
    }
    let s = heap.str_repr(val);
    heap.alloc_str_dynamic(s)
}
