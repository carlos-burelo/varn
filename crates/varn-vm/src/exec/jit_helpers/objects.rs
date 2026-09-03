//! Object-level operations that are not field access: key enumeration,
//! `in`, merge and rest.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_object_keys_stub(ctx: *mut ExecCtx, val_tag: u64, val_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let val = VmValue::from_raw_parts(val_tag, val_payload);
        match crate::exec::collections::object_keys(val, &mut ctx_ref.heap) {
            Ok(keys_val) => ctx_ref.jit_native_result = keys_val,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_op_in_stub(
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
        ctx_ref.jit_native_result =
            VmValue::from_bool(crate::exec::advanced::op_in(a, b, &ctx_ref.heap));
    }
}

pub(crate) extern "C" fn jit_object_merge_stub(
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
        match crate::exec::collections::object_merge(a, b, &mut ctx_ref.heap) {
            Ok(res) => ctx_ref.jit_native_result = res,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_object_rest(ctx: *mut ExecCtx, ip_before: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let base = ctx_ref.frames[frame_idx].base;
        let code = &closure_ref.proto.chunk.code;

        let mut temp_ip = ip_before;
        let w1 = code[temp_ip];
        temp_ip += 1;
        let w2 = code[temp_ip];
        temp_ip += 1;
        let src = (w1 & 0xFF) as usize;
        let skip_count = (w2 >> 8) as usize;
        let mut skip_keys = Vec::with_capacity(skip_count);
        for _ in 0..skip_count {
            let k_idx = code[temp_ip] as usize;
            temp_ip += 1;
            let key_nv = closure_ref.constants[k_idx];
            skip_keys.push(ctx_ref.heap.str_val(key_nv).unwrap_or_else(|| {
                closure_ref.proto.chunk.constants[k_idx]
                    .as_str()
                    .unwrap_or("")
                    .into()
            }));
        }
        let obj = ctx_ref.stack[base + src];
        match ctx_ref.exec_object_rest(obj, &skip_keys) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
