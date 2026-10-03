use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// `extern "C" fn(ctx, callee_tag, callee_payload, window: *const VmValue, argc)`
/// — the SSA lowering's spread call. `window[0..argc]` holds the (already
/// `WrapSpread`-marked) arguments; each `Spread` wrapper — and, as the
/// interpreter's `exec_call_spread_reg` does, each bare array — expands into
/// the callee's argument list. Runs through the same [`ExecCtx::invoke`] as
/// `jit_invoke_window`, so there is still one invocation.
#[varn_op_macros::jit_slow(field = "jit_call_spread_window")]
pub(crate) extern "C" fn jit_call_spread_window(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    window: *const VmValue,
    argc: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let args = std::slice::from_raw_parts(window, argc);
        let mut expanded = Vec::with_capacity(argc + 1);
        expanded.push(callee);
        crate::exec::calls::expand_spread_args(&ctx_ref.heap, args.iter().copied(), &mut expanded);
        match ctx_ref.invoke(callee, &expanded) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
/// `extern "C" fn(ctx, callee_tag, callee_payload, window: *const VmValue, argc)`
/// — the SSA lowering's fallback for a call whose caller has no VM activation
/// of its own to keep its argument window in. `window[0]` is the callee
/// placeholder, `window[1..]` the arguments (built on the caller's native
/// stack); this runs the callee through the SAME [`ExecCtx::invoke`] as
/// `jit_invoke_dynamic`, so there is still one invocation, and writes the
/// boxed result to `ctx.jit_native_result`.
#[varn_op_macros::jit_slow(field = "jit_invoke_window")]
pub(crate) extern "C" fn jit_invoke_window(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    window: *const VmValue,
    argc: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let window = std::slice::from_raw_parts(window, argc);
        match ctx_ref.invoke(callee, window) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
