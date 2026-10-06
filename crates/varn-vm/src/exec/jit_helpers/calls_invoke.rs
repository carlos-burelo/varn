use super::calls_static::{hand_off, settle, Handoff};
use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::exec::host::isolates::Invoked;
use crate::value::VmValue;

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
#[varn_op_macros::jit_slow(field = "jit_invoke_window")]
pub(crate) extern "C" fn jit_invoke_window(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    window: *const VmValue,
    argc: usize,
    out: *mut usize,
) -> usize {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let window = std::slice::from_raw_parts(window, argc);
        let handoff = match ctx_ref.invoke_pushing(callee, window) {
            Ok(Invoked::Value(v)) => Handoff::Ran(Ok(v)),
            Ok(Invoked::Pushed(depth)) => hand_off(ctx_ref, depth, out),
            Err(e) => Handoff::Ran(Err(e)),
        };
        settle(ctx_ref, handoff)
    }
}
