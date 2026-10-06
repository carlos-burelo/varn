


use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;








#[varn_op_macros::jit_slow(field = "jit_call_native_window")]
pub(crate) extern "C" fn jit_call_native_window(
    ctx: *mut ExecCtx,
    fn_addr: usize,
    op_id: u64,
    window: *const VmValue,
    total: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let f = resolve_native(ctx_ref, fn_addr, op_id);
        ctx_ref.record_call_native(f, None);
        let args = std::slice::from_raw_parts(window, total);
        ctx_ref.jit_native_result = match ctx_ref.invoke_native(f, args) {
            Ok(v) => v,
            Err(err) => jit_propagate_error(ctx_ref, crate::error::RuntimeError::from(err)),
        };
    }
}




unsafe fn resolve_native(
    ctx_ref: &mut ExecCtx,
    fn_addr: usize,
    op_id: u64,
) -> varn_types::NativeFn {
    if fn_addr != 0 {
        return std::mem::transmute::<usize, varn_types::NativeFn>(fn_addr);
    }
    match varn_builtins::native_op_fn(op_id) {
        Some(f) => f,
        None => jit_propagate_error(
            ctx_ref,
            crate::error::RuntimeError::new(format!("CallNativeOp: unknown op-id {op_id}")),
        ),
    }
}
