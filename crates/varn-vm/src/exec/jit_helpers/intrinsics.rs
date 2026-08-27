//! Intrinsic dispatch and the string intrinsics compiled code calls
//! directly, without the stack-window flush and reload a generic call needs.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

pub(crate) extern "C" fn jit_dispatch_intrinsic(
    ctx: *mut ExecCtx,
    wire_byte: usize,
    args_start: usize,
    arg_count: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let required = args_start + arg_count;
        if ctx_ref.stack.len() < required {
            ctx_ref.stack.resize(required, VmValue::null());
        }
        let args = &ctx_ref.stack[args_start..required];
        match crate::exec::intrinsics::dispatch(wire_byte as u8, args, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// Dedicated fast path for `charCodeAt(pos)` / `codePointAt(pos)`.
/// Takes the receiver and position directly — no stack-window staging,
/// no flush/reload of all live boxed registers.
///
/// A negative `pos` is out of range, not position zero: see
/// [`crate::exec::intrinsics::str`] for why all three implementations of this
/// operation had to agree on that.
pub(crate) extern "C" fn jit_str_char_code_at(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    pos: VmValue,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let heap = &mut ctx_ref.heap;
        let signed = heap.as_int(pos);
        if signed < 0 {
            return VmValue::from_int(-1);
        }
        let idx = signed as usize;

        // SSO string — always ASCII, bytes packed in the VmValue itself.
        if receiver.is_sso() {
            let mut buf = [0u8; 5];
            let len = receiver.sso_copy_bytes(&mut buf);
            return if idx < len {
                VmValue::from_int(buf[idx] as i64)
            } else {
                VmValue::from_int(-1)
            };
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
                return VmValue::from_int(code.unwrap_or(-1));
            }
        }
        VmValue::from_int(-1)
    }
}

/// Address of `receiver`'s bytes, when it is a heap string whose content is
/// ASCII — and therefore one where byte index equals character index, so a
/// `charCodeAt` is a single indexed byte load.
///
/// `0` for everything else: an SSO string (its bytes live inside the `VmValue`
/// and have no address), a non-ASCII string (byte index is not character
/// index), or a receiver that is not a string at all. Callers treat `0` as
/// "use the general path", so a new `HeapStr` variant is a missed
/// optimisation, never a wrong answer.
///
/// # Safety of the returned pointer
///
/// It borrows into heap-owned memory and stays valid only while nothing
/// allocates: an allocation can grow the slot `Vec` and move an `Inline`
/// string's bytes with it, and a collection can free the object outright. The
/// JIT resolves this exactly once, in the preheader of a loop region proven
/// allocation-free (`scan::loop_regions`), and the back-edge safepoint zeroes
/// the cache if a collection did run.
pub(crate) extern "C" fn jit_str_ascii_bytes(ctx: *mut ExecCtx, receiver: VmValue) -> *const u8 {
    unsafe {
        match ascii_view(&(*ctx).heap, receiver) {
            Some(s) => s.as_ptr(),
            None => std::ptr::null(),
        }
    }
}

/// Byte length of the view [`jit_str_ascii_bytes`] returned for the same
/// receiver, or `0` when that call rejected it.
pub(crate) extern "C" fn jit_str_ascii_len(ctx: *mut ExecCtx, receiver: VmValue) -> i64 {
    unsafe { ascii_view(&(*ctx).heap, receiver).map_or(0, |s| s.len() as i64) }
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

/// Dedicated fast path for `substring(start, end?)`.
/// Avoids the generic intrinsic dispatcher's flush/reload overhead.
pub(crate) extern "C" fn jit_str_substring_intrinsic(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    start: VmValue,
    end: VmValue,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let heap = &mut ctx_ref.heap;

        // Delegate to the intrinsic dispatcher with a minimal stack slice.
        // This is still cheaper than the generic path because the JIT caller
        // did NOT flush/reload all boxed registers.
        let args = [receiver, start, end];
        match crate::exec::intrinsics::str::dispatch(
            varn_core::intrinsic_ops::str::StrOp::Substring as u8,
            &args,
            heap,
        ) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// Dedicated fast path for `slice(start, end?)`.
/// Avoids the generic intrinsic dispatcher's flush/reload overhead.
pub(crate) extern "C" fn jit_str_slice_intrinsic(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    start: VmValue,
    end: VmValue,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let heap = &mut ctx_ref.heap;

        let args = [receiver, start, end];
        match crate::exec::intrinsics::str::dispatch(
            varn_core::intrinsic_ops::str::StrOp::Slice as u8,
            &args,
            heap,
        ) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

#[inline(always)]
unsafe fn borrow_str_fast<'a>(
    v: VmValue,
    heap: &'a Heap,
    buf: &'a mut [u8; 5],
) -> Option<&'a str> {
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
/// Avoids the generic intrinsic dispatcher's flush/reload overhead.
pub(crate) extern "C" fn jit_str_starts_with_intrinsic(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    search: VmValue,
) -> VmValue {
    unsafe {
        let heap = &(*ctx).heap;
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            return VmValue::from_bool(s.as_bytes().starts_with(n.as_bytes()));
        }
        let ctx_ref = &mut *ctx;
        let args = [receiver, search];
        match crate::exec::intrinsics::str::dispatch(
            varn_core::intrinsic_ops::str::StrOp::StartsWith as u8,
            &args,
            &mut ctx_ref.heap,
        ) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// Dedicated fast path for `endsWith(search)`.
/// Avoids the generic intrinsic dispatcher's flush/reload overhead.
pub(crate) extern "C" fn jit_str_ends_with_intrinsic(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    search: VmValue,
) -> VmValue {
    unsafe {
        let heap = &(*ctx).heap;
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            return VmValue::from_bool(s.as_bytes().ends_with(n.as_bytes()));
        }
        let ctx_ref = &mut *ctx;
        let args = [receiver, search];
        match crate::exec::intrinsics::str::dispatch(
            varn_core::intrinsic_ops::str::StrOp::EndsWith as u8,
            &args,
            &mut ctx_ref.heap,
        ) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// Dedicated fast path for `includes(search)`.
/// Avoids the generic intrinsic dispatcher's flush/reload overhead.
pub(crate) extern "C" fn jit_str_includes_intrinsic(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    search: VmValue,
) -> VmValue {
    unsafe {
        let heap = &(*ctx).heap;
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            return VmValue::from_bool(varn_types::str_util::find_bytes(s, n).is_some());
        }
        let ctx_ref = &mut *ctx;
        let args = [receiver, search];
        match crate::exec::intrinsics::str::dispatch(
            varn_core::intrinsic_ops::str::StrOp::Includes as u8,
            &args,
            &mut ctx_ref.heap,
        ) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// Dedicated fast path for `indexOf(search)`.
/// Avoids the generic intrinsic dispatcher's flush/reload overhead.
pub(crate) extern "C" fn jit_str_index_of_intrinsic(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    search: VmValue,
) -> VmValue {
    unsafe {
        let heap = &(*ctx).heap;
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            if n.is_empty() {
                return VmValue::from_int(0);
            }
            let is_ascii = receiver.is_sso()
                || heap
                    .get(receiver.as_heap_idx())
                    .map(|o| match o {
                        HeapObj::Str(h) => h.is_ascii_cached(),
                        _ => false,
                    })
                    .unwrap_or(false);
            let idx = varn_types::str_util::find_bytes(s, n)
                .map(|b| varn_types::str_util::byte_to_char_idx(s, is_ascii, b))
                .unwrap_or(-1);
            return VmValue::from_int(idx);
        }
        let ctx_ref = &mut *ctx;
        let args = [receiver, search];
        match crate::exec::intrinsics::str::dispatch(
            varn_core::intrinsic_ops::str::StrOp::IndexOf as u8,
            &args,
            &mut ctx_ref.heap,
        ) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// Dedicated fast path for `lastIndexOf(search)`.
/// Avoids the generic intrinsic dispatcher's flush/reload overhead.
pub(crate) extern "C" fn jit_str_last_index_of_intrinsic(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    search: VmValue,
) -> VmValue {
    unsafe {
        let heap = &(*ctx).heap;
        let mut b1 = [0u8; 5];
        let mut b2 = [0u8; 5];
        if let (Some(s), Some(n)) = (
            borrow_str_fast(receiver, heap, &mut b1),
            borrow_str_fast(search, heap, &mut b2),
        ) {
            let is_ascii = receiver.is_sso()
                || heap
                    .get(receiver.as_heap_idx())
                    .map(|o| match o {
                        HeapObj::Str(h) => h.is_ascii_cached(),
                        _ => false,
                    })
                    .unwrap_or(false);
            if n.is_empty() {
                return VmValue::from_int(varn_types::str_util::char_len(s, is_ascii) as i64);
            }
            let idx = varn_types::str_util::rfind_bytes(s, n)
                .map(|b| varn_types::str_util::byte_to_char_idx(s, is_ascii, b))
                .unwrap_or(-1);
            return VmValue::from_int(idx);
        }
        let ctx_ref = &mut *ctx;
        let args = [receiver, search];
        match crate::exec::intrinsics::str::dispatch(
            varn_core::intrinsic_ops::str::StrOp::LastIndexOf as u8,
            &args,
            &mut ctx_ref.heap,
        ) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

