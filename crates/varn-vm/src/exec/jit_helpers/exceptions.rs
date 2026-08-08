//! Exception handling from compiled code: pushing and popping try handlers,
//! and throwing.

use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_push_try(ctx: *mut ExecCtx, catch_ip: usize, err_reg: u32) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_depth = ctx_ref.frames.len();
        crate::exec::exceptions::push_try(
            &mut ctx_ref.try_handlers,
            catch_ip,
            frame_depth,
            err_reg as u8,
        );
    }
}

pub(crate) extern "C" fn jit_pop_try(ctx: *mut ExecCtx) {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::exceptions::pop_try(&mut ctx_ref.try_handlers);
    }
}

pub(crate) extern "C" fn jit_throw(ctx: *mut ExecCtx, error: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let err =
            crate::exec::exceptions::build_thrown_error(error, &ctx_ref.heap, &ctx_ref.frames);
        let handler = ctx_ref.try_handlers.pop();
        ctx_ref.jit_panic_exception_handler = handler;
        ctx_ref.jit_panic_exception_error = Some(err.thrown.unwrap_or(VmValue::null()));
        ctx_ref.jit_panic_exception_err_obj = Some(err);

        let buf = ctx_ref.jit_jmp_buf;
        if !buf.is_null() {
            crate::exec::ctx::my_longjmp(buf, 1);
        } else {
            panic!("JIT exception triggered but no jump buffer registered");
        }
    }
}
