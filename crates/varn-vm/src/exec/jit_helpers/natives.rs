//! Calling native (Rust) functions from compiled code, plus the stack-growth
//! helper their argument windows depend on.
//!
//! K3-faseA: los helpers que direccionaban la ventana contigua quedan
//! invalidados (solo los invocaba código generado). `jit_is_native_fn` no
//! toca el layout y sigue real. Fase B: restaurar de git.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

macro_rules! bailed {
    () => {
        unreachable!("K3-faseA: helper de código compilado; ver FRAME_LAYOUT_V2_JIT_BAIL")
    };
}

pub(crate) extern "C" fn jit_ensure_stack_capacity(ctx: *mut ExecCtx, required_len: usize) {
    let _ = (ctx, required_len);
    bailed!()
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
    let _ = (ctx, callee, arg_start, arg_count);
    bailed!()
}

/// JIT helper for `CallNativeOp`: resolve the stable op-id to its native fn and
/// invoke it. The compiled caller has already flushed `[receiver, args...]` to
/// the home slots of registers `reg_start..reg_start + total` in activation
/// `act_id`; this reads them back as a contiguous `VmValue` slice, exactly the
/// layout the macro-generated wrapper expects — mirroring the interpreter's
/// `CallNativeOp` arm (`call_native_with_receiver`).
pub(crate) extern "C" fn jit_call_native_op(
    ctx: *mut ExecCtx,
    op_id: u64,
    act_id: usize,
    reg_start: usize,
    total: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = match varn_builtins::native_op_fn(op_id) {
            Some(f) => call_native_from_homes(ctx_ref, f, act_id, reg_start, total),
            None => jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new(format!("CallNativeOp: unknown op-id {op_id}")),
            ),
        };
        ctx_ref.jit_native_result = res;
    }
}

/// `CallNativeOp` with the target already resolved at JIT-compile time —
/// no per-call op-id hash lookup.
pub(crate) extern "C" fn jit_call_native_fnptr(
    ctx: *mut ExecCtx,
    fn_addr: usize,
    act_id: usize,
    reg_start: usize,
    total: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let f: varn_types::NativeFn = std::mem::transmute(fn_addr);
        let res = call_native_from_homes(ctx_ref, f, act_id, reg_start, total);
        ctx_ref.jit_native_result = res;
    }
}

/// Gather `[reg_start, reg_start + total)` of activation `act_id` into a
/// `VmValue` slice (per-class home reads via `FrameStore`) and invoke `f`.
unsafe fn call_native_from_homes(
    ctx_ref: &mut ExecCtx,
    f: varn_types::NativeFn,
    act_id: usize,
    reg_start: usize,
    total: usize,
) -> VmValue {
    ctx_ref.record_call_native(f, None);
    let args = ctx_ref.stack.box_range(act_id, reg_start, total);
    match ctx_ref.invoke_native(f, &args) {
        Ok(v) => v,
        Err(msg) => jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(msg)),
    }
}
