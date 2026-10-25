use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

#[varn_macros::jit_slow(field = "str_concat")]
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

#[varn_macros::jit_slow(field = "str_slice")]
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

#[varn_macros::jit_slow(field = "str_length")]
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

#[varn_macros::jit_slow(field = "str_substring")]
pub(crate) extern "C" fn jit_str_substring(
    ctx: *mut ExecCtx,
    s_tag: u64,
    s_payload: u64,
    start_tag: u64,
    start_payload: u64,
    has_end: u64,
    end_tag: u64,
    end_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let heap = &ctx_ref.heap;
        let s_val = VmValue::from_raw_parts(s_tag, s_payload);
        let start_val = VmValue::from_raw_parts(start_tag, start_payload);
        let end_val = VmValue::from_raw_parts(end_tag, end_payload);
        let mut sso_buf = [0u8; varn_types::vm_value::SSO_MAX_LEN];
        let (text, ascii): (&str, bool) = if s_val.is_sso() {
            let s = s_val.sso_as_str(&mut sso_buf);
            (s, s.is_ascii())
        } else if s_val.is_heap() {
            match heap.get(s_val.as_heap()) {
                Some(crate::heap::HeapObj::Str(hs)) => (hs.as_str(), hs.is_ascii()),
                _ => {
                    let msg = format!("expected str, got {s_val:?}");
                    jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(msg));
                    return;
                }
            }
        } else {
            let msg = format!("expected str, got {s_val:?}");
            jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(msg));
            return;
        };
        let int_arg = |v: VmValue| {
            if v.is_int() {
                Some(v.as_int())
            } else {
                None
            }
        };
        let Some(start) = int_arg(start_val) else {
            let msg = format!("expected int, got {start_val:?}");
            jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(msg));
            return;
        };
        let end_opt: Option<i64> = if has_end == 0 || end_val.is_null() {
            None
        } else if let Some(n) = int_arg(end_val) {
            Some(n)
        } else {
            let msg = format!("expected int, got {end_val:?}");
            jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(msg));
            return;
        };
        let total = varn_types::str_util::char_len(text, ascii);
        let (s, e) = match varn_types::str_util::substring_bounds(total, start, end_opt) {
            Ok(range) => range,
            Err(message) => {
                jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(message));
                return;
            }
        };
        let (bs, be) = varn_types::str_util::char_range_to_bytes(text, ascii, s, e);
        ctx_ref.jit_native_result = ctx_ref.heap.alloc_str_view_of(s_val, bs, be - bs);
    }
}
