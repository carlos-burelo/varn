use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

#[varn_op_macros::jit_slow(field = "gc_safepoint")]
pub(crate) extern "C" fn jit_gc_safepoint(ctx: *mut ExecCtx) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.gc_backedge_safepoint();
    }
}

#[varn_op_macros::jit_slow(field = "assert_not_null")]
pub(crate) extern "C" fn jit_assert_not_null(ctx: *mut ExecCtx, val: VmValue) {
    if let Err(e) = crate::exec::advanced::assert_not_null(val) {
        unsafe {
            let ctx_ref = &mut *ctx;
            jit_propagate_error(ctx_ref, e);
        }
    }
}

#[varn_op_macros::jit_slow(field = "close_upvalue")]
pub(crate) extern "C" fn jit_close_upvalue(ctx: *mut ExecCtx, lowest: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;

        let alloc = ctx_ref.frames[frame_idx].base;
        ctx_ref.close_upvalues_from_reg(alloc, lowest);
    }
}
