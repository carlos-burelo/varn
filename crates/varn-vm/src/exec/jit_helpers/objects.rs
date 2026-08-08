//! Object-level operations that are not field access: key enumeration,
//! `in`, merge and rest.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_object_keys_stub(ctx: *mut ExecCtx, val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::collections::object_keys(val, &mut ctx_ref.heap) {
            Ok(keys_val) => keys_val,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_op_in_stub(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;
        VmValue::from_bool(crate::exec::advanced::op_in(a, b, &ctx_ref.heap))
    }
}

pub(crate) extern "C" fn jit_object_merge_stub(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::collections::object_merge(a, b, &mut ctx_ref.heap) {
            Ok(res) => res,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_object_rest(ctx: *mut ExecCtx, ip_before: usize) -> VmValue {
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
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

