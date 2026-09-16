//! Calling native (Rust) functions from compiled code, plus the stack-growth
//! helper their argument windows depend on.
//!
//! K3-faseA: los helpers que direccionaban la ventana contigua quedan
//! invalidados (solo los invocaba código generado). `jit_is_native_fn` no
//! toca el layout y sigue real. Fase B: restaurar de git.

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
    let _ = (ctx, op_id, args_start, total);
    bailed!()
}

/// `CallNativeOp` with the target already resolved at JIT-compile time —
/// no per-call op-id hash lookup.
pub(crate) extern "C" fn jit_call_native_fnptr(
    ctx: *mut ExecCtx,
    fn_addr: usize,
    args_start: usize,
    total: usize,
) {
    let _ = (ctx, fn_addr, args_start, total);
    bailed!()
}
