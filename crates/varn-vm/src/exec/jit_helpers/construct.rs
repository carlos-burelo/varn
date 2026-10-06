






use crate::exec::ctx::ExecCtx;

#[inline(always)]
pub(crate) unsafe fn jit_propagate_error(ctx: &mut ExecCtx, e: crate::error::RuntimeError) -> ! {
    ctx.jit_panic_exception_error =
        Some(crate::exec::exceptions::thrown_value_for(&e, &mut ctx.heap));
    ctx.jit_panic_exception_err_obj = Some(e);
    let buf = ctx.jit_jmp_buf;
    if !buf.is_null() {
        crate::exec::ctx::my_longjmp(buf, 1);
    }
    panic!("JIT error: no jump buffer");
}
