//! Object construction reached from compiled code, and the error escape.
//!
//! `new X(...)` is the one call shape that must allocate before it can call,
//! so it does not fit the ordinary call helpers. `jit_propagate_error` sits
//! alongside it because it is the exit every helper in this tree takes when
//! it cannot return normally.

use crate::exec::ctx::ExecCtx;

#[inline(always)]
pub(crate) unsafe fn jit_propagate_error(ctx: &mut ExecCtx, e: crate::error::RuntimeError) -> ! {
    let handler = ctx
        .jit_panic_exception_handler
        .take()
        .or_else(|| ctx.try_handlers.pop());
    ctx.jit_panic_exception_handler = handler;
    ctx.jit_panic_exception_error =
        Some(crate::exec::exceptions::thrown_value_for(&e, &mut ctx.heap));
    ctx.jit_panic_exception_err_obj = Some(e);
    let buf = ctx.jit_jmp_buf;
    if !buf.is_null() {
        crate::exec::ctx::my_longjmp(buf, 1);
    }
    panic!("JIT error: no jump buffer");
}

pub(crate) extern "C" fn jit_alloc_instance_fast(
    ctx: *mut ExecCtx,
    class_id: u32,
    payload_size: u32,
) -> u64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let inst = varn_types::value::InstanceRef::alloc_with_layout(class_id, payload_size);
        ctx_ref.heap.alloc(crate::heap::HeapObj::Instance(inst)) as u64
    }
}
