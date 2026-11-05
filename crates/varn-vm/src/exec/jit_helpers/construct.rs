use crate::exec::ctx::ExecCtx;

#[inline(always)]
pub(crate) unsafe fn jit_propagate_error(ctx: &mut ExecCtx, mut e: crate::error::RuntimeError) {
    if e.frames.is_empty() {
        e.frames = crate::exec::exceptions::collect_frames(&ctx.frames);
    }
    ctx.jit_native_result = crate::exec::exceptions::thrown_value_for(&e, &mut ctx.heap);
    ctx.jit_panic_exception_error = Some(ctx.jit_native_result);
    ctx.jit_panic_exception_err_obj = Some(e);
    ctx.jit_error_flag = 1;
}
