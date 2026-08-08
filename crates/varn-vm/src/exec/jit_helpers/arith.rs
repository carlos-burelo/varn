//! Arithmetic, unary and bitwise operators that compiled code calls out
//! for rather than inlining — the cases that can allocate (decimals), can
//! fail (division by zero) or need the full numeric tower.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_negate(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::negate(v, &mut ctx_ref.heap)
    }
}

pub(crate) extern "C" fn jit_logical_not(_ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    crate::exec::compare::logical_not(v)
}

pub(crate) extern "C" fn jit_div(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::arith::div(a, b, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_modulo(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::arith::modulo(a, b, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_pow(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::arith::pow(a, b, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_bitand(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::bit_and(a, b, &mut (*ctx).heap) }
}

pub(crate) extern "C" fn jit_bitor(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::bit_or(a, b, &mut (*ctx).heap) }
}

pub(crate) extern "C" fn jit_bitxor(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::bit_xor(a, b, &mut (*ctx).heap) }
}

pub(crate) extern "C" fn jit_shl(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::shl(a, b, &mut (*ctx).heap) }
}

pub(crate) extern "C" fn jit_shr(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::shr(a, b, &mut (*ctx).heap) }
}

pub(crate) extern "C" fn jit_ushr(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe { crate::exec::arith::ushr(a, b, &mut (*ctx).heap) }
}

