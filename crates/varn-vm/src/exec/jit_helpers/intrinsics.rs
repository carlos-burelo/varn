//! Intrinsic dispatch and the string intrinsics compiled code calls
//! directly, without the stack-window flush and reload a generic call needs.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
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
pub(crate) extern "C" fn jit_str_char_code_at(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    pos: VmValue,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let heap = &mut ctx_ref.heap;
        let idx = heap.as_int(pos).max(0) as usize;

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

/// Ultra-lean fast path for `charCodeAt(pos)` in JIT-compiled code.
///
/// Differences from `jit_str_char_code_at`:
/// - `pos` is a raw i64 (already unboxed by the JIT's int kind), not a VmValue.
/// - Returns a raw i64 result, not a VmValue — the JIT keeps it as `K::Int`.
/// - Skips SSO checks (irrelevant for loop-heavy benchmarks on heap strings).
/// - The common path is: heap lookup → ascii check → byte load — 3 branches.
#[inline(never)]
pub(crate) extern "C" fn jit_str_char_code_at_fast(
    ctx: *mut ExecCtx,
    receiver: VmValue,
    pos: i64,
) -> i64 {
    unsafe {
        let heap = &(*ctx).heap;
        let idx = pos.max(0) as usize;

        if receiver.is_heap() {
            if let Some(crate::heap::HeapObj::Str(h)) = heap.get(receiver.as_heap_idx()) {
                if h.is_ascii_cached() {
                    return h.as_str().as_bytes().get(idx).map_or(-1, |&b| b as i64);
                }
                // Non-ASCII or UNKNOWN: populate cache and use the right path.
                h.is_ascii();
                let s = h.as_str();
                return if h.is_ascii_cached() {
                    s.as_bytes().get(idx).map_or(-1, |&b| b as i64)
                } else {
                    s.chars().nth(idx).map_or(-1, |c| c as i64)
                };
            }
        }
        // SSO fallback (rare in hot loops on large strings).
        if receiver.is_sso() {
            let mut buf = [0u8; 5];
            let len = receiver.sso_copy_bytes(&mut buf);
            return if idx < len { buf[idx] as i64 } else { -1 };
        }
        -1
    }
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

