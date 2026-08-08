use crate::heap::{Heap, HeapObj, HeapStr};
use crate::strbuf::StrBuf;
use crate::value::VmValue;
use std::rc::Rc;

/// Left operands at or above this length seed an extensible buffer, so a
/// `s = s + x` accumulation appends in place from then on. Shorter results
/// stay `Shared` — one-off concats (e.g. map keys) shouldn't pay the
/// copy-on-materialize of a buffer view.
const EXT_SEED_LEN: usize = 16;

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

pub(crate) fn str_concat(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
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

    // The `"prefix" + <int>` shape, built once instead of staged through a
    // `StrBuf` and a zeroed `[u8; INLINE_STR_CAP]`. Declines to anything it
    // cannot serve, including an `Ext` left operand — but the accumulation
    // path above has already claimed those.
    if let Some(v) = heap.alloc_str_concat_inline(a, b) {
        return v;
    }

    // Render both operands into one buffer that starts on the stack: strings
    // borrow, scalars format in place. A short result (the common
    // `"prefix" + int`) therefore costs a single allocation — the `Rc<str>`
    // handed back — where it used to cost a scratch `String` plus a copy into
    // that `Rc<str>`.
    let mut out = StrBuf::new();
    heap.str_repr_into(a, &mut out);
    let a_len = out.len();
    heap.str_repr_into(b, &mut out);
    let total = out.len();
    if a_len >= EXT_SEED_LEN {
        // Seeding an extensible buffer: this one has to be owned and growable.
        let flag = if out.as_str().is_ascii() {
            crate::heap::ascii_flag::YES
        } else {
            crate::heap::ascii_flag::NO
        };
        let mut owned = out.into_string();
        owned.reserve(total);
        return heap.alloc_str_view(HeapStr::ext(
            Rc::new(std::cell::UnsafeCell::new(owned)),
            total,
            flag,
        ));
    }
    heap.alloc_str_dynamic(out.as_str())
}

pub(crate) fn to_string(val: VmValue, heap: &mut Heap) -> VmValue {
    let s = heap.str_repr(val);
    heap.alloc_str_dynamic(s)
}
