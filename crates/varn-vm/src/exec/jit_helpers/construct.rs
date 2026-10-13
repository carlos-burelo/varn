use crate::exec::ctx::ExecCtx;

#[inline(always)]
pub(crate) unsafe fn jit_propagate_error(ctx: &mut ExecCtx, e: crate::error::RuntimeError) {
    ctx.jit_native_result = crate::exec::exceptions::thrown_value_for(&e, &mut ctx.heap);
    ctx.jit_panic_exception_error = Some(ctx.jit_native_result);
    ctx.jit_panic_exception_err_obj = Some(e);
    ctx.jit_error_flag = 1;
}
