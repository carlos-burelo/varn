//! Intrinsic dispatch, and the string fast paths compiled code calls
//! directly, without the stack-window flush and reload a generic call needs.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

/// Fase B: the compiled caller has flushed `[receiver, args...]` to the home
/// slots of registers `reg_start..reg_start + arg_count` in activation
/// `act_id`; this gathers them back through `FrameStore` and dispatches.
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
        if std::env::var_os("VARN_HOME_TRACE").is_some() {
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

/// Dedicated fast path for `charCodeAt(pos)` / `codePointAt(pos)`.
/// Takes the receiver and position directly — no stack-window staging,
/// no flush/reload of all live boxed registers.
///
/// A negative `pos` is out of range, not position zero, as in the native
/// `str.charCodeAt` the interpreter runs.
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

        // SSO string — always ASCII, bytes packed in the VmValue itself.
        if receiver.is_sso() {
            let mut buf = [0u8; 5];
            let len = receiver.sso_copy_bytes(&mut buf);
            return if idx < len { buf[idx] as i64 } else { -1 };
        }

        if receiver.is_heap() {
            if let Some(crate::heap::HeapObj::Str(h)) = heap.get(receiver.as_heap_idx()) {
                let s = h.as_str();
                let code = if h.is_ascii_cached() {
                    s.as_bytes().get(idx).map(|&b| b as i64)
                } else {
                    // Ensure the ASCII cache is populated for next call.
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

/// The shared decision behind both accessors, so they cannot disagree about
/// which receivers are byte-indexable.
#[inline]
fn ascii_view(heap: &crate::heap::Heap, receiver: VmValue) -> Option<&str> {
    if !receiver.is_heap() {
        return None;
    }
    let Some(crate::heap::HeapObj::Str(h)) = heap.get(receiver.as_heap_idx()) else {
        return None;
    };
    // `is_ascii()` computes and memoises; `is_ascii_cached()` alone would
    // answer "no" for a string nobody has classified yet.
    h.is_ascii().then(|| h.as_str())
}

#[inline(always)]
unsafe fn borrow_str_fast<'a>(v: VmValue, heap: &'a Heap, buf: &'a mut [u8; 5]) -> Option<&'a str> {
    if v.is_sso() {
        return Some(v.sso_as_str(buf));
    }
    if v.is_heap() {
        if let Some(HeapObj::Str(h)) = heap.get(v.as_heap_idx()) {
            return Some(h.as_str());
        }
    }
    None
}

/// Dedicated fast path for `startsWith(search)`.
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
            if let Some(HeapObj::Str(h)) = heap.get(receiver.as_heap_idx()) {
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
            if let (Some(HeapObj::Str(h1)), Some(HeapObj::Str(h2))) = (
                heap.get(receiver.as_heap_idx()),
                heap.get(search.as_heap_idx()),
            ) {
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

/// Dedicated fast path for `endsWith(search)`.
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
            if let Some(HeapObj::Str(h)) = heap.get(receiver.as_heap_idx()) {
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
            if let (Some(HeapObj::Str(h1)), Some(HeapObj::Str(h2))) = (
                heap.get(receiver.as_heap_idx()),
                heap.get(search.as_heap_idx()),
            ) {
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
