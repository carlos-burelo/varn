use crate::heap::{Heap, HeapObj, HeapStr, INLINE_STR_CAP};
use crate::strbuf::StrBuf;
use crate::value::VmValue;
use std::rc::Rc;

pub(crate) fn fast_length(val: VmValue, heap: &Heap) -> Option<VmValue> {
    if val.is_sso() {
        return Some(VmValue::from_i32(val.sso_char_len() as i32));
    }
    if val.is_heap() {
        match heap.get(val.as_heap()) {
            Some(HeapObj::Str(s)) => {
                return Some(VmValue::from_i32(s.char_len() as i32));
            }
            Some(HeapObj::Array(arr)) => {
                return Some(VmValue::from_int(arr.len() as i64));
            }
            Some(
                HeapObj::Tuple(_)
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
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => {}
        }
    }
    None
}

pub(crate) fn str_concat(a: VmValue, b: VmValue, heap: &mut Heap) -> VmValue {
    if is_empty_str(a, heap) && is_str(b, heap) {
        return b;
    }
    if is_empty_str(b, heap) && is_str(a, heap) {
        return a;
    }
    if a.is_sso() && b.is_sso() {
        let la = a.sso_len();
        let lb = b.sso_len();
        let total = la + lb;
        if total <= varn_types::vm_value::SSO_MAX_LEN {
            let packed = a.raw_payload() | (b.raw_payload() << (la * 8));
            return VmValue::from_sso_raw(total, packed);
        }
    }

    if a.is_heap()
        && let Some(HeapObj::Str(hs)) = heap.get(a.as_heap())
        && let HeapStr::Ext { buf, len, ascii } = hs
        && hs.is_tip()
    {
        let buf = Rc::clone(buf);
        let len = *len;
        let a_ascii = ascii.get();

        let sb = heap.str_repr(b);

        let flag = match a_ascii {
            crate::heap::ascii_flag::NO => crate::heap::ascii_flag::NO,
            crate::heap::ascii_flag::YES if sb.is_ascii() => crate::heap::ascii_flag::YES,
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

    if let Some(v) = heap.alloc_str_concat_inline(a, b) {
        return v;
    }

    let mut out = StrBuf::new();
    heap.str_repr_into(a, &mut out);
    heap.str_repr_into(b, &mut out);
    fresh_ext(heap, out.into_string())
}

fn is_str(v: VmValue, heap: &Heap) -> bool {
    v.is_sso() || (v.is_heap() && matches!(heap.get(v.as_heap()), Some(HeapObj::Str(_))))
}

fn is_empty_str(v: VmValue, heap: &Heap) -> bool {
    if v.is_sso() {
        return v.sso_len() == 0;
    }
    v.is_heap() && matches!(heap.get(v.as_heap()), Some(HeapObj::Str(s)) if s.byte_len() == 0)
}

pub(crate) fn str_concat_parts(parts: &[VmValue], heap: &mut Heap) -> VmValue {
    if parts.is_empty() {
        return VmValue::try_from_sso("").unwrap_or_else(VmValue::null);
    }
    if parts.len() == 2 {
        return str_concat(parts[0], parts[1], heap);
    }
    if let Some(v) = sso_pack(parts) {
        return v;
    }
    if let Some(v) = try_append_tip(heap, parts[0], &parts[1..]) {
        return v;
    }
    let mut out = StrBuf::new();
    for v in parts {
        heap.str_repr_into(*v, &mut out);
    }
    fresh_ext(heap, out.into_string())
}

fn sso_pack(parts: &[VmValue]) -> Option<VmValue> {
    let mut total = 0usize;
    for v in parts {
        if !v.is_sso() {
            return None;
        }
        total += v.sso_len();
    }
    if total > varn_types::vm_value::SSO_MAX_LEN {
        return None;
    }
    let mut payload = 0u64;
    let mut shift = 0u32;
    for v in parts {
        payload |= v.raw_payload() << shift;
        shift += (v.sso_len() * 8) as u32;
    }
    Some(VmValue::from_sso_raw(total, payload))
}

fn try_append_tip(heap: &mut Heap, first: VmValue, rest: &[VmValue]) -> Option<VmValue> {
    if !first.is_heap() {
        return None;
    }
    let (buf, len, a_ascii) = match heap.get(first.as_heap()) {
        Some(HeapObj::Str(hs)) => {
            if let HeapStr::Ext { buf, len, ascii } = hs {
                if !hs.is_tip() {
                    return None;
                }
                (Rc::clone(buf), *len, ascii.get())
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let mut sb = String::new();
    for v in rest {
        sb.push_str(&heap.str_repr(*v));
    }
    let flag = match a_ascii {
        crate::heap::ascii_flag::NO => crate::heap::ascii_flag::NO,
        crate::heap::ascii_flag::YES if sb.is_ascii() => crate::heap::ascii_flag::YES,
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
    Some(heap.alloc_str_view(HeapStr::ext(buf, target_len, flag)))
}

fn fresh_ext(heap: &mut Heap, s: String) -> VmValue {
    if s.len() <= INLINE_STR_CAP {
        return heap.alloc_str_view(HeapStr::inline(&s));
    }
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
    if val.is_heap()
        && let Some(HeapObj::Str(_)) = heap.get(val.as_heap())
    {
        return val;
    }
    let s = heap.str_repr(val);
    heap.alloc_str_dynamic(s)
}
