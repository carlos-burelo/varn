//! String operations compiled code calls directly: concatenation,
//! slicing and length.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_str_concat(
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
        ctx_ref.jit_native_result = crate::exec::strings::str_concat(a, b, &mut ctx_ref.heap);
    }
}

pub(crate) extern "C" fn jit_str_slice(
    ctx: *mut ExecCtx,
    s_tag: u64,
    s_payload: u64,
    idx_tag: u64,
    idx_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let s = VmValue::from_raw_parts(s_tag, s_payload);
        let idx = VmValue::from_raw_parts(idx_tag, idx_payload);
        match ctx_ref.exec_str_slice(s, idx) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_str_length(ctx: *mut ExecCtx, v_tag: u64, v_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(v_tag, v_payload);
        match ctx_ref.exec_str_length(v) {
            Ok(len) => ctx_ref.jit_native_result = len,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
