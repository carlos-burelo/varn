//! Calling native (Rust) functions from compiled code, plus the stack-growth
//! helper their argument windows depend on.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_ensure_stack_capacity(ctx: *mut ExecCtx, required_len: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let required_cap = required_len + 32;
        let stack_len = ctx_ref.stack.len();
        if ctx_ref.stack.capacity() < required_cap {
            ctx_ref.stack.reserve(required_cap - stack_len);
        }
        if stack_len < required_len {
            ctx_ref.stack.set_len(required_len);
            let ptr = ctx_ref.stack.as_mut_ptr();
            for i in stack_len..required_len {
                std::ptr::write(ptr.add(i), VmValue::null());
            }
        }
    }
}

pub(crate) extern "C" fn jit_is_native_fn(ctx: *mut ExecCtx, callee: VmValue) -> usize {
    unsafe {
        let ctx_ref = &*ctx;
        if callee.is_heap() {
            if let Some(crate::heap::HeapObj::NativeFn(..)) = ctx_ref.heap.get(callee.as_heap_idx())
            {
                return 1;
            }
        }
        0
    }
}

pub(crate) extern "C" fn jit_call_native_fast(
    ctx: *mut ExecCtx,
    callee: VmValue,
    arg_start: usize,
    arg_count: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        if callee.is_heap() {
            let heap_obj = ctx_ref.heap.get(callee.as_heap_idx());
            if let Some(crate::heap::HeapObj::NativeFn(f, name)) = heap_obj {
                let f = *f;
                let name = *name;
                ctx_ref.record_call_native(f, Some(name));
                let arg_base = base + arg_start;

                let result = if arg_count <= 1 {
                    ctx_ref.invoke_native(f, &[])
                } else {
                    let actual_count = arg_count - 1;
                    if actual_count <= 8 {
                        let mut buf = [VmValue::null(); 8];
                        buf[..actual_count].copy_from_slice(
                            &ctx_ref.stack[(arg_base + 1)..(arg_base + 1 + actual_count)],
                        );
                        ctx_ref.invoke_native(f, &buf[..actual_count])
                    } else {
                        let vargs: Vec<VmValue> = (1..=actual_count)
                            .map(|i| ctx_ref.stack[arg_base + i])
                            .collect();
                        ctx_ref.invoke_native(f, &vargs)
                    }
                };
                match result {
                    Ok(v) => return v,
                    Err(msg) => {
                        let e = crate::error::RuntimeError::new(msg);
                        jit_propagate_error(ctx_ref, e);
                    }
                }
            }
        }
        panic!("jit_call_native_fast called with non-native callee");
    }
}

/// JIT helper for `CallNativeOp`: resolve the stable op-id to its native fn and
/// invoke it. The stack slice `[receiver, args...]` is already laid out
/// contiguously at `args_start` (absolute), which is exactly the layout the
/// macro-generated wrapper expects — so this mirrors the interpreter's
/// `CallNativeOp` arm (and its `call_native_with_receiver` path).
pub(crate) extern "C" fn jit_call_native_op(
    ctx: *mut ExecCtx,
    op_id: u64,
    args_start: usize,
    total: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = match varn_builtins::native_op_fn(op_id) {
            Some(f) => call_native_from_stack(ctx_ref, f, args_start, total),
            None => {
                jit_propagate_error(
                    ctx_ref,
                    crate::error::RuntimeError::new(format!("CallNativeOp: unknown op-id {op_id}")),
                );
            }
        };
        ctx_ref.jit_native_result = res;
    }
}

/// `CallNativeOp` with the target already resolved at JIT-compile time —
/// no per-call op-id hash lookup.
pub(crate) extern "C" fn jit_call_native_fnptr(
    ctx: *mut ExecCtx,
    fn_addr: usize,
    args_start: usize,
    total: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let f: varn_types::NativeFn = std::mem::transmute(fn_addr);
        let res = call_native_from_stack(ctx_ref, f, args_start, total);
        ctx_ref.jit_native_result = res;
    }
}

unsafe fn call_native_from_stack(
    ctx_ref: &mut ExecCtx,
    f: varn_types::NativeFn,
    args_start: usize,
    total: usize,
) -> VmValue {
    ctx_ref.record_call_native(f, None);
    let result = if total <= 16 {
        let mut buf = [VmValue::null(); 16];
        std::ptr::copy_nonoverlapping(
            ctx_ref.stack.as_ptr().add(args_start),
            buf.as_mut_ptr(),
            total,
        );
        ctx_ref.invoke_native(f, &buf[..total])
    } else {
        let v: Vec<VmValue> = (0..total).map(|i| ctx_ref.stack[args_start + i]).collect();
        ctx_ref.invoke_native(f, &v)
    };
    match result {
        Ok(v) => v,
        Err(msg) => jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(msg)),
    }
}
