//! Object construction reached from compiled code, and the error escape.
//!
//! `new X(...)` is the one call shape that must allocate before it can call,
//! so it does not fit the ordinary call helpers. `jit_propagate_error` sits
//! alongside it because it is the exit every helper in this tree takes when
//! it cannot return normally.

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
