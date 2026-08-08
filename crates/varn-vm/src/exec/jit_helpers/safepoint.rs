//! GC safepoints and the scope-exit obligations compiled code owes the
//! interpreter: closing upvalues, and the null assertion a non-null type
//! still has to prove at runtime.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// Loop back-edge safepoint. Mirrors the interpreter's `OpCode::Loop`
/// handler: collect the nursery (and pace the major GC) so long call-free
/// allocating loops don't overflow the nursery into the old generation.
/// Takes no `VmValue` arguments by design — the caller has flushed every VM
/// register to the stack, so all roots are visible and get reloaded after.
pub(crate) extern "C" fn jit_gc_safepoint(ctx: *mut ExecCtx) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.gc_backedge_safepoint();
    }
}

pub(crate) extern "C" fn jit_assert_not_null(ctx: *mut ExecCtx, val: VmValue) {
    if let Err(e) = crate::exec::advanced::assert_not_null(val) {
        unsafe {
            let ctx_ref = &mut *ctx;
            jit_propagate_error(ctx_ref, e);
        }
    }
}

pub(crate) extern "C" fn jit_close_upvalue(ctx: *mut ExecCtx, lowest: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let base = ctx_ref.frames[frame_idx].base;
        ctx_ref.close_upvalues_above(base + lowest);
    }
}
