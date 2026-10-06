use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

#[varn_op_macros::jit_slow(field = "dispatch_intrinsic")]
pub(crate) extern "C" fn jit_dispatch_intrinsic(
    ctx: *mut ExecCtx,
    wire_byte: usize,
    act_id: usize,
    reg_start: usize,
    arg_count: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = ctx_ref.stack.box_range(act_id, reg_start, arg_count);
        if crate::home_trace::enabled() {
            let fname = ctx_ref
                .frames
                .last()
                .and_then(|f| f.closure().proto.name.clone());
            let tags: Vec<String> = args
                .iter()
                .map(|v| format!("{:#x}/{:#x}", v.raw_tag(), v.raw_payload()))
                .collect();
            eprintln!("INTRINSIC {fname:?} wire={wire_byte:#x} args={tags:?}");
        }
        match crate::exec::intrinsics::dispatch(wire_byte as u8, &args) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

#[varn_op_macros::jit_slow(field = "intrinsic_window")]
pub(crate) extern "C" fn jit_intrinsic_window(
    ctx: *mut ExecCtx,
    wire_byte: usize,
    window: *const VmValue,
    count: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = std::slice::from_raw_parts(window, count);
        match crate::exec::intrinsics::dispatch(wire_byte as u8, args) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

#[varn_op_macros::jit_slow(field = "str_char_code_at")]
pub(crate) extern "C" fn jit_str_char_code_at(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
    pos_tag: u64,
    pos_payload: u64,
) -> i64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let heap = &mut ctx_ref.heap;
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        let pos = VmValue::from_raw_parts(pos_tag, pos_payload);
        let signed = heap.as_int(pos);
        if signed < 0 {
            return -1;
        }
        let idx = signed as usize;

        if receiver.is_sso() {
            let mut buf = [0u8; 5];
            let len = receiver.sso_copy_bytes(&mut buf);
            return if idx < len { buf[idx] as i64 } else { -1 };
        }

        if receiver.is_heap() {
            if let Some(crate::heap::HeapObj::Str(h)) = heap.get(receiver.as_heap()) {
                let s = h.as_str();
                let code = if h.is_ascii_cached() {
                    s.as_bytes().get(idx).map(|&b| b as i64)
                } else {
                    h.is_ascii();
                    if h.is_ascii_cached() {
                        s.as_bytes().get(idx).map(|&b| b as i64)
                    } else {
                        s.chars().nth(idx).map(|c| c as i64)
                    }
                };
                return code.unwrap_or(-1);
            }
        }
        -1
    }
}

#[varn_op_macros::jit_slow(field = "str_ascii_bytes")]
pub(crate) extern "C" fn jit_str_ascii_bytes(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
) -> *const u8 {
    unsafe {
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        match ascii_view(&(*ctx).heap, receiver) {
            Some(s) => s.as_ptr(),
            None => std::ptr::null(),
        }
    }
}

#[varn_op_macros::jit_slow(field = "str_ascii_len")]
pub(crate) extern "C" fn jit_str_ascii_len(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
) -> i64 {
    unsafe {
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        ascii_view(&(*ctx).heap, receiver).map_or(0, |s| s.len() as i64)
    }
}

#[inline]
fn ascii_view(heap: &crate::heap::Heap, receiver: VmValue) -> Option<&str> {
    if !receiver.is_heap() {
        return None;
    }
    let Some(crate::heap::HeapObj::Str(h)) = heap.get(receiver.as_heap()) else {
        return None;
    };

    h.is_ascii().then(|| h.as_str())
}

#[inline(always)]
unsafe fn borrow_str_fast<'a>(v: VmValue, heap: &'a Heap, buf: &'a mut [u8; 5]) -> Option<&'a str> {
    if v.is_sso() {
        return Some(v.sso_as_str(buf));
    }
    if v.is_heap() {
        if let Some(HeapObj::Str(h)) = heap.get(v.as_heap()) {
            return Some(h.as_str());
        }
    }
    None
}

#[varn_op_macros::jit_slow(field = "str_starts_with")]
pub(crate) extern "C" fn jit_str_starts_with(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
    search_tag: u64,
    search_payload: u64,
) -> u64 {
    unsafe {
        let heap = &(*ctx).heap;
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        let search = VmValue::from_raw_parts(search_tag, search_payload);
        if receiver.is_sso() && search.is_sso() {
            let r_len = receiver.sso_len();
            let s_len = search.sso_len();
            if s_len == 0 {
                return 1;
            }
            if r_len < s_len {
                return 0;
            }
            let shift = (s_len * 8) as u32;
            let mask = if shift >= 64 {
                u64::MAX
            } else {
                (1u64 << shift) - 1
            };
            return if (receiver.raw_payload() & mask) == (search.raw_payload() & mask) {
                1
            } else {
                0
            };
        }
        if receiver.is_heap() && search.is_sso() {
            if let Some(HeapObj::Str(h)) = heap.get(receiver.as_heap()) {
                let n_len = search.sso_len();
                let s_bytes = h.as_str().as_bytes();
                if s_bytes.len() >= n_len {
                    let mut b2 = [0u8; 5];
                    search.sso_copy_bytes(&mut b2);
                    return if s_bytes[..n_len] == b2[..n_len] {
                        1
                    } else {
                        0
                    };
                }
                return 0;
            }
        }
        if receiver.is_heap() && search.is_heap() {
            if let (Some(HeapObj::Str(h1)), Some(HeapObj::Str(h2))) =
                (heap.get(receiver.as_heap()), heap.get(search.as_heap()))
            {
                return if h1.as_str().as_bytes().starts_with(h2.as_str().as_bytes()) {
                    1
                } else {
                    0
                };
            }
        }
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            return if s.as_bytes().starts_with(n.as_bytes()) {
                1
            } else {
                0
            };
        }
        jit_propagate_error(
            &mut *ctx,
            crate::error::RuntimeError::new("startsWith: receiver and argument must be strings"),
        )
    }
}

#[varn_op_macros::jit_slow(field = "str_ends_with")]
pub(crate) extern "C" fn jit_str_ends_with(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
    search_tag: u64,
    search_payload: u64,
) -> u64 {
    unsafe {
        let heap = &(*ctx).heap;
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        let search = VmValue::from_raw_parts(search_tag, search_payload);
        if receiver.is_sso() && search.is_sso() {
            let r_len = receiver.sso_len();
            let s_len = search.sso_len();
            if s_len == 0 {
                return 1;
            }
            if r_len < s_len {
                return 0;
            }
            let r_offset = (r_len - s_len) * 8;
            let r_shifted = receiver.raw_payload() >> r_offset;
            let shift = (s_len * 8) as u32;
            let mask = if shift >= 64 {
                u64::MAX
            } else {
                (1u64 << shift) - 1
            };
            return if (r_shifted & mask) == (search.raw_payload() & mask) {
                1
            } else {
                0
            };
        }
        if receiver.is_heap() && search.is_sso() {
            if let Some(HeapObj::Str(h)) = heap.get(receiver.as_heap()) {
                let n_len = search.sso_len();
                let s_bytes = h.as_str().as_bytes();
                if s_bytes.len() >= n_len {
                    let mut b2 = [0u8; 5];
                    search.sso_copy_bytes(&mut b2);
                    return if s_bytes[s_bytes.len() - n_len..] == b2[..n_len] {
                        1
                    } else {
                        0
                    };
                }
                return 0;
            }
        }
        if receiver.is_heap() && search.is_heap() {
            if let (Some(HeapObj::Str(h1)), Some(HeapObj::Str(h2))) =
                (heap.get(receiver.as_heap()), heap.get(search.as_heap()))
            {
                return if h1.as_str().as_bytes().ends_with(h2.as_str().as_bytes()) {
                    1
                } else {
                    0
                };
            }
        }
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            return if s.as_bytes().ends_with(n.as_bytes()) {
                1
            } else {
                0
            };
        }
        jit_propagate_error(
            &mut *ctx,
            crate::error::RuntimeError::new("endsWith: receiver and argument must be strings"),
        )
    }
}

#[varn_op_macros::jit_slow(field = "str_index_of")]
pub(crate) extern "C" fn jit_str_index_of(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
    search_tag: u64,
    search_payload: u64,
) -> i64 {
    unsafe {
        let heap = &(*ctx).heap;
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        let search = VmValue::from_raw_parts(search_tag, search_payload);
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            use varn_types::str_util::{byte_to_char_idx, find_bytes};
            if n.is_empty() {
                return 0;
            }
            let ascii = s.is_ascii();
            return find_bytes(s, n)
                .map(|b| byte_to_char_idx(s, ascii, b))
                .unwrap_or(-1);
        }
        jit_propagate_error(
            &mut *ctx,
            crate::error::RuntimeError::new("indexOf: receiver and argument must be strings"),
        )
    }
}

#[varn_op_macros::jit_slow(field = "str_includes")]
pub(crate) extern "C" fn jit_str_includes(
    ctx: *mut ExecCtx,
    recv_tag: u64,
    recv_payload: u64,
    search_tag: u64,
    search_payload: u64,
) -> u64 {
    unsafe {
        let heap = &(*ctx).heap;
        let receiver = VmValue::from_raw_parts(recv_tag, recv_payload);
        let search = VmValue::from_raw_parts(search_tag, search_payload);
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            use varn_types::str_util::find_bytes;
            return if find_bytes(s, n).is_some() { 1 } else { 0 };
        }
        jit_propagate_error(
            &mut *ctx,
            crate::error::RuntimeError::new("includes: receiver and argument must be strings"),
        )
    }
}

#[allow(clippy::too_many_arguments)]
#[varn_op_macros::jit_slow(field = "str_split")]
pub(crate) extern "C" fn jit_str_split(
    ctx: *mut ExecCtx,
    s_tag: u64,
    s_payload: u64,
    argc: u64,
    sep_tag: u64,
    sep_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let mut buf = [0u8; 5];
        let s_val = VmValue::from_raw_parts(s_tag, s_payload);
        let Some(text) = borrow_str_fast(s_val, &ctx_ref.heap, &mut buf) else {
            jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new("split: receiver must be a string"),
            );
        };

        let text = text.to_owned();
        let sep: Option<String> = if argc == 0 {
            None
        } else {
            let sep_val = VmValue::from_raw_parts(sep_tag, sep_payload);
            let mut sbuf = [0u8; 5];
            match borrow_str_fast(sep_val, &ctx_ref.heap, &mut sbuf) {
                Some(sep) => Some(sep.to_owned()),
                None => jit_propagate_error(
                    ctx_ref,
                    crate::error::RuntimeError::new("split: separator must be a string"),
                ),
            }
        };
        let mut out = Vec::new();
        match sep.as_deref() {
            Some(sep) if sep.len() == 1 => {
                let byte = sep.as_bytes()[0];
                for p in text.split(byte as char) {
                    out.push(ctx_ref.heap.alloc_str(p));
                }
            }
            Some(sep) => {
                for p in text.split(sep) {
                    out.push(ctx_ref.heap.alloc_str(p));
                }
            }
            None => {
                let mut cbuf = [0u8; 4];
                for c in text.chars() {
                    out.push(ctx_ref.heap.alloc_str(c.encode_utf8(&mut cbuf)));
                }
            }
        }
        ctx_ref.jit_native_result = ctx_ref
            .heap
            .alloc_array_repr(false, varn_types::vm_value::ArrayRepr::boxed(out));
    }
}

#[allow(clippy::too_many_arguments)]
#[varn_op_macros::jit_slow(field = "str_slice_range")]
pub(crate) extern "C" fn jit_str_slice_range(
    ctx: *mut ExecCtx,
    s_tag: u64,
    s_payload: u64,
    start_tag: u64,
    start_payload: u64,
    has_end: u64,
    end_tag: u64,
    end_payload: u64,
) {
    use varn_types::str_util::{char_len, char_range_to_bytes};
    unsafe {
        let ctx_ref = &mut *ctx;
        let mut buf = [0u8; 5];
        let s_val = VmValue::from_raw_parts(s_tag, s_payload);
        let start_val = VmValue::from_raw_parts(start_tag, start_payload);
        let end_val = VmValue::from_raw_parts(end_tag, end_payload);
        let text = match borrow_str_fast(s_val, &ctx_ref.heap, &mut buf) {
            Some(t) => t.to_owned(),
            None => jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new("slice: receiver must be a string"),
            ),
        };
        let Some(start) = start_val.is_int().then(|| start_val.as_int()) else {
            jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new("slice: bounds must be ints"),
            );
        };
        let end: Option<i64> = if has_end == 0 {
            None
        } else if end_val.is_int() {
            Some(end_val.as_int())
        } else {
            jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new("slice: bounds must be ints"),
            );
        };
        let out = if text.is_ascii() {
            let len = text.len() as i64;
            let si = normalize_idx(start, len).min(text.len());
            let ei = normalize_idx(end.unwrap_or(len), len)
                .min(text.len())
                .max(si);
            text[si..ei].to_owned()
        } else {
            let len = char_len(&text, false);
            let si = normalize_idx(start, len as i64).min(len);
            let ei = normalize_idx(end.unwrap_or(len as i64), len as i64)
                .min(len)
                .max(si);
            let (bs, be) = char_range_to_bytes(&text, false, si, ei);
            text[bs..be].to_owned()
        };
        ctx_ref.jit_native_result = ctx_ref.heap.alloc_str(&out);
    }
}

#[inline]
fn normalize_idx(idx: i64, len: i64) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        idx as usize
    }
}
