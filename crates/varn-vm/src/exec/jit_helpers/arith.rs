//! Arithmetic, unary and bitwise operators that compiled code calls out
//! for rather than inlining — the cases that can allocate (decimals), can
//! fail (division by zero) or need the full numeric tower.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// `extern "C" fn() -> u64` — the live `ExecCtx` pointer, recovered from the
/// thread-local `clif_link::CtxGuard` sets for the run's whole dynamic
/// extent. Zero arguments on purpose: any real parameter would have to be
/// materialized on the caller's hot path, which is exactly the cost a LEAF
/// lowering's overflow guard (`clif::emit::guard_overflow`) is calling this
/// to avoid. Called only from the guard's cold `raise` block, so the extra
/// indirection is paid on overflow, never on the arithmetic itself.
pub(crate) extern "C" fn jit_current_exec_ctx() -> u64 {
    crate::clif_link::current_ctx_ptr() as u64
}

pub(crate) extern "C" fn jit_negate(ctx: *mut ExecCtx, v_tag: u64, v_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(v_tag, v_payload);
        match crate::exec::arith::negate(v, &mut ctx_ref.heap) {
            Ok(r) => ctx_ref.jit_native_result = r,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_logical_not(_ctx: *mut ExecCtx, v_tag: u64, v_payload: u64) -> u64 {
    let v = VmValue::from_raw_parts(v_tag, v_payload);
    if crate::exec::compare::logical_not(v).is_truthy() {
        1
    } else {
        0
    }
}

pub(crate) extern "C" fn jit_div(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        match crate::exec::arith::div(a, b, &mut ctx_ref.heap) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_modulo(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        match crate::exec::arith::modulo(a, b, &mut ctx_ref.heap) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_pow(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        match crate::exec::arith::pow(a, b, &mut ctx_ref.heap) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_bitand(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        (*ctx).jit_native_result = crate::exec::arith::bit_and(a, b, &mut (*ctx).heap);
    }
}

pub(crate) extern "C" fn jit_bitor(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        (*ctx).jit_native_result = crate::exec::arith::bit_or(a, b, &mut (*ctx).heap);
    }
}

pub(crate) extern "C" fn jit_bitxor(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        (*ctx).jit_native_result = crate::exec::arith::bit_xor(a, b, &mut (*ctx).heap);
    }
}

pub(crate) extern "C" fn jit_shl(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        (*ctx).jit_native_result = crate::exec::arith::shl(a, b, &mut (*ctx).heap);
    }
}

pub(crate) extern "C" fn jit_shr(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        (*ctx).jit_native_result = crate::exec::arith::shr(a, b, &mut (*ctx).heap);
    }
}

pub(crate) extern "C" fn jit_ushr(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) {
    unsafe {
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        (*ctx).jit_native_result = crate::exec::arith::ushr(a, b, &mut (*ctx).heap);
    }
}
