use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj, HeapStr};
use crate::value::VmValue;
use varn_core::intrinsic_ops::str::StrOp;
use varn_types::str_util::{byte_to_char_idx, char_len, char_range_to_bytes};

fn not_a_string() -> RuntimeError {
    RuntimeError::new("str intrinsic: operand is not a string")
}

/// Borrow a string operand straight from the heap (or the SSO buffer) with
/// its cached ASCII state. No refcount traffic — only valid while `heap` is
/// not mutated. SSO strings are ASCII by construction.
#[inline]
fn view<'a>(v: VmValue, heap: &'a Heap, buf: &'a mut [u8; 5]) -> VmResult<(&'a str, bool)> {
    if v.is_sso() {
        return Ok((v.sso_as_str(buf), true));
    }
    if v.is_heap() {
        if let Some(HeapObj::Str(h)) = heap.get(v.as_heap_idx()) {
            return Ok((h.as_str(), h.is_ascii_cached()));
        }
    }
    Err(not_a_string())
}

/// Owned cheap handle (`Rc` clone / inline SSO bytes) for ops that allocate:
/// the heap borrow must end before `alloc_str_dynamic`.
enum StrHandle {
    Heap(HeapStr),
    Sso([u8; 5], usize),
}

impl StrHandle {
    fn get(v: VmValue, heap: &Heap) -> VmResult<Self> {
        if v.is_sso() {
            let mut sso = [0u8; 5];
            let len = v.sso_copy_bytes(&mut sso);
            return Ok(StrHandle::Sso(sso, len));
        }
        if v.is_heap() {
            if let Some(HeapObj::Str(h)) = heap.get(v.as_heap_idx()) {
                return Ok(StrHandle::Heap(h.clone()));
            }
        }
        Err(not_a_string())
    }

    #[inline]
    fn as_str(&self) -> &str {
        match self {
            StrHandle::Heap(h) => h.as_str(),
            // Safety: SSO payloads only admit bytes <= 127 (`try_from_sso`).
            StrHandle::Sso(buf, len) => unsafe { std::str::from_utf8_unchecked(&buf[..*len]) },
        }
    }

    #[inline]
    fn is_ascii(&self) -> bool {
        match self {
            StrHandle::Heap(h) => h.is_ascii_cached(),
            StrHandle::Sso(..) => true,
        }
    }
}

/// Optional int argument: absent or null both mean "not provided".
#[inline]
fn opt_int(args: &[VmValue], i: usize, heap: &Heap) -> Option<i64> {
    args.get(i).filter(|v| !v.is_null()).map(|&v| heap.as_int(v))
}

#[inline]
fn req_int(args: &[VmValue], i: usize, heap: &Heap) -> i64 {
    args.get(i).map(|&v| heap.as_int(v)).unwrap_or(0)
}

#[inline]
fn arg(args: &[VmValue], i: usize) -> VmValue {
    args.get(i).copied().unwrap_or_else(VmValue::null)
}

#[inline]
fn normalize_idx(idx: i64, len: i64) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        idx as usize
    }
}

/// Substring result: the full range returns the receiver unchanged (strings
/// are immutable), heap sources go through the zero-copy `Slice` path.
#[inline]
fn alloc_sub(
    heap: &mut Heap,
    recv: VmValue,
    this: &StrHandle,
    s: &str,
    bs: usize,
    be: usize,
) -> VmValue {
    if bs == 0 && be == s.len() {
        return recv;
    }
    match this {
        StrHandle::Heap(h) => heap.alloc_substring(h, bs, be),
        StrHandle::Sso(..) => heap.alloc_str_dynamic(&s[bs..be]),
    }
}

pub fn dispatch(op: u8, args: &[VmValue], heap: &mut Heap) -> VmResult<VmValue> {
    let recv = args
        .first()
        .copied()
        .ok_or_else(|| RuntimeError::new("str intrinsic: missing receiver"))?;

    if recv.is_heap() {
        if let Some(HeapObj::Str(h)) = heap.get(recv.as_heap_idx()) {
            h.is_ascii_cached();
        }
    }

    // Non-allocating ops: borrow the operands directly from the heap.
    let mut recv_buf = [0u8; 5];
    let mut needle_buf = [0u8; 5];
    match op {
        o if o == StrOp::CharCodeAt as u8 || o == StrOp::CodePointAt as u8 => {
            let (s, ascii) = view(recv, heap, &mut recv_buf)?;
            let pos = req_int(args, 1, heap).max(0) as usize;
            let code = if ascii {
                s.as_bytes().get(pos).map(|&b| b as i64)
            } else {
                s.chars().nth(pos).map(|c| c as i64)
            };
            return Ok(VmValue::from_int(code.unwrap_or(-1)));
        }
        o if o == StrOp::CharCode as u8 => {
            let (s, _) = view(recv, heap, &mut recv_buf)?;
            let code = s.chars().next().map(|c| c as i64).unwrap_or(-1);
            return Ok(VmValue::from_int(code));
        }
        o if o == StrOp::IndexOf as u8 => {
            let (s, ascii) = view(recv, heap, &mut recv_buf)?;
            let (n, _) = view(arg(args, 1), heap, &mut needle_buf)?;
            if n.is_empty() {
                return Ok(VmValue::from_int(0));
            }
            let idx = s
                .find(n)
                .map(|b| byte_to_char_idx(s, ascii, b))
                .unwrap_or(-1);
            return Ok(VmValue::from_int(idx));
        }
        o if o == StrOp::LastIndexOf as u8 => {
            let (s, ascii) = view(recv, heap, &mut recv_buf)?;
            let (n, _) = view(arg(args, 1), heap, &mut needle_buf)?;
            if n.is_empty() {
                return Ok(VmValue::from_int(char_len(s, ascii) as i64));
            }
            let idx = s
                .rfind(n)
                .map(|b| byte_to_char_idx(s, ascii, b))
                .unwrap_or(-1);
            return Ok(VmValue::from_int(idx));
        }
        o if o == StrOp::StartsWith as u8 => {
            let (s, _) = view(recv, heap, &mut recv_buf)?;
            let (n, _) = view(arg(args, 1), heap, &mut needle_buf)?;
            return Ok(VmValue::from_bool(s.starts_with(n)));
        }
        o if o == StrOp::EndsWith as u8 => {
            let (s, _) = view(recv, heap, &mut recv_buf)?;
            let (n, _) = view(arg(args, 1), heap, &mut needle_buf)?;
            return Ok(VmValue::from_bool(s.ends_with(n)));
        }
        o if o == StrOp::Includes as u8 => {
            let (s, _) = view(recv, heap, &mut recv_buf)?;
            let (n, _) = view(arg(args, 1), heap, &mut needle_buf)?;
            return Ok(VmValue::from_bool(s.contains(n)));
        }
        _ => {}
    }

    // Allocating ops: own a cheap handle so the heap borrow ends before the
    // result allocation.
    let this = StrHandle::get(recv, heap)?;
    let s = this.as_str();
    let ascii = this.is_ascii();

    match op {
        o if o == StrOp::At as u8 => {
            let len = char_len(s, ascii) as i64;
            let idx = req_int(args, 1, heap);
            let idx = if idx < 0 { len + idx } else { idx };
            if idx < 0 || idx >= len {
                return Ok(VmValue::null());
            }
            let (bs, be) = char_range_to_bytes(s, ascii, idx as usize, idx as usize + 1);
            Ok(alloc_sub(heap, recv, &this, s, bs, be))
        }
        o if o == StrOp::Substring as u8 => {
            let len = char_len(s, ascii);
            let start = req_int(args, 1, heap);
            let end = opt_int(args, 2, heap);
            let a = (start.max(0) as usize).min(len);
            let b = end.map(|e| e.max(0) as usize).unwrap_or(len).min(len);
            let (si, ei) = if a <= b { (a, b) } else { (b, a) };
            let (bs, be) = char_range_to_bytes(s, ascii, si, ei);
            Ok(alloc_sub(heap, recv, &this, s, bs, be))
        }
        o if o == StrOp::Slice as u8 => {
            let len = char_len(s, ascii);
            let start = req_int(args, 1, heap);
            let end = opt_int(args, 2, heap);
            let si = normalize_idx(start, len as i64).min(len);
            let ei = normalize_idx(end.unwrap_or(len as i64), len as i64)
                .min(len)
                .max(si);
            let (bs, be) = char_range_to_bytes(s, ascii, si, ei);
            Ok(alloc_sub(heap, recv, &this, s, bs, be))
        }
        o if o == StrOp::Substr as u8 => {
            let len = char_len(s, ascii);
            let start = req_int(args, 1, heap);
            let length = opt_int(args, 2, heap);
            let st = if start < 0 {
                (len as i64 + start).max(0) as usize
            } else {
                (start as usize).min(len)
            };
            let count = length.map(|c| c.max(0) as usize).unwrap_or(len - st);
            let ei = (st + count).min(len);
            let (bs, be) = char_range_to_bytes(s, ascii, st, ei);
            Ok(alloc_sub(heap, recv, &this, s, bs, be))
        }
        _ => Err(RuntimeError::new(format!("str intrinsic: unknown op {op}"))),
    }
}
