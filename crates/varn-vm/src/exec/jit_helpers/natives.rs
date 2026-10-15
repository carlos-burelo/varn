use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

#[varn_macros::jit_slow(field = "jit_call_native_window")]
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
        match ctx_ref.invoke_native(f, args) {
            Ok(v) => {
                ctx_ref.jit_native_result = v;
            }
            Err(err) => {
                jit_propagate_error(ctx_ref, crate::error::RuntimeError::from(err));
            }
        }
    }
}

unsafe fn resolve_native(
    ctx_ref: &mut ExecCtx,
    fn_addr: usize,
    op_id: u64,
) -> varn_types::NativeFn {
    if fn_addr != 0 {
        return unsafe { std::mem::transmute::<usize, varn_types::NativeFn>(fn_addr) };
    }
    match crate::exec::contract::native_op(op_id) {
        Some(f) => f,
        None => unsafe {
            jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new(format!("CallNativeOp: unknown op-id {op_id}")),
            );
            unknown_op_native
        },
    }
}

fn unknown_op_native(
    _ctx: &mut dyn varn_types::NativeCtx,
    _args: &[VmValue],
) -> Result<VmValue, varn_types::NativeError> {
    Err(varn_types::NativeError::from("CallNativeOp: unknown op-id"))
}
