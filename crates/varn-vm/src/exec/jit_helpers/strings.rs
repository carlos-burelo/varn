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

pub(crate) extern "C" fn jit_str_length(
    ctx: *mut ExecCtx,
    v_tag: u64,
    v_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(v_tag, v_payload);
        match ctx_ref.exec_str_length(v) {
            Ok(len) => ctx_ref.jit_native_result = len,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_str_starts_with(
    ctx: *mut ExecCtx,
    this_tag: u64,
    this_payload: u64,
    prefix_tag: u64,
    prefix_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let this_val = VmValue::from_raw_parts(this_tag, this_payload);
        let prefix_val = VmValue::from_raw_parts(prefix_tag, prefix_payload);
        let mut buf_a = [0u8; 5];
        let mut buf_b = [0u8; 5];
        let s_opt = if this_val.is_sso() {
            Some(this_val.sso_as_str(&mut buf_a))
        } else if this_val.is_heap() {
            if let Some(crate::heap::HeapObj::Str(hs)) = ctx_ref.heap.get(this_val.as_heap_idx()) {
                Some(hs.as_str())
            } else {
                None
            }
        } else {
            None
        };
        let p_opt = if prefix_val.is_sso() {
            Some(prefix_val.sso_as_str(&mut buf_b))
        } else if prefix_val.is_heap() {
            if let Some(crate::heap::HeapObj::Str(hs)) = ctx_ref.heap.get(prefix_val.as_heap_idx()) {
                Some(hs.as_str())
            } else {
                None
            }
        } else {
            None
        };
        let res = match (s_opt, p_opt) {
            (Some(s), Some(p)) => s.starts_with(p),
            _ => false,
        };
        ctx_ref.jit_native_result = VmValue::from_bool(res);
    }
}

pub(crate) extern "C" fn jit_str_ends_with(
    ctx: *mut ExecCtx,
    this_tag: u64,
    this_payload: u64,
    suffix_tag: u64,
    suffix_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let this_val = VmValue::from_raw_parts(this_tag, this_payload);
        let suffix_val = VmValue::from_raw_parts(suffix_tag, suffix_payload);
        let mut buf_a = [0u8; 5];
        let mut buf_b = [0u8; 5];
        let s_opt = if this_val.is_sso() {
            Some(this_val.sso_as_str(&mut buf_a))
        } else if this_val.is_heap() {
            if let Some(crate::heap::HeapObj::Str(hs)) = ctx_ref.heap.get(this_val.as_heap_idx()) {
                Some(hs.as_str())
            } else {
                None
            }
        } else {
            None
        };
        let p_opt = if suffix_val.is_sso() {
            Some(suffix_val.sso_as_str(&mut buf_b))
        } else if suffix_val.is_heap() {
            if let Some(crate::heap::HeapObj::Str(hs)) = ctx_ref.heap.get(suffix_val.as_heap_idx()) {
                Some(hs.as_str())
            } else {
                None
            }
        } else {
            None
        };
        let res = match (s_opt, p_opt) {
            (Some(s), Some(p)) => s.ends_with(p),
            _ => false,
        };
        ctx_ref.jit_native_result = VmValue::from_bool(res);
    }
}

