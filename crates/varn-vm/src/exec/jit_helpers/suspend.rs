//! Suspension: `await`, `yield`, `spawn`, and the shared parking mechanism
//! underneath them.
//!
//! All of these leave compiled code through the OUTERMOST clif frame's jump
//! buffer (`ExecCtx::jit_suspend_buf`), not the per-frame one — an
//! intermediate clif frame cannot park in the middle of a native call.

use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// Leave clif code with frame `frame_idx` parked at `resume_ip`, so the
/// interpreter picks that function up from there once the suspension the
/// caller already recorded in `vm_suspend` is resolved.
pub(super) unsafe fn jit_suspend_at(ctx: &mut ExecCtx, frame_idx: usize, resume_ip: usize) -> ! {
    ctx.frames[frame_idx].ip = resume_ip;
    // The unwind handler patches the TOP frame's ip from this field. We have
    // already parked the right frame, and it is not the top one — the module
    // that suspended left its own frame above ours — so leave it clear.
    ctx.jit_panic_suspend_resume_ip = None;
    // The OUTERMOST clif frame's buffer: `jit_jmp_buf` is per-frame now, and
    // a suspension cannot stop at an intermediate one (see
    // `execute_jit_frame`).
    let buf = ctx.jit_suspend_buf;
    if buf.is_null() {
        panic!("JIT suspend triggered but no jump buffer registered");
    }
    crate::exec::ctx::my_longjmp(buf, 2)
}

pub(crate) extern "C" fn jit_spawn(ctx: *mut ExecCtx, task_val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.exec_spawn(task_val).unwrap()
    }
}

pub(crate) extern "C" fn jit_await(
    ctx: *mut ExecCtx,
    fut: VmValue,
    dest_reg: u32,
    resume_ip: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        ctx_ref.frames[frame_idx].ip = resume_ip;
        ctx_ref.vm_suspend = Some(crate::exec::VmSuspend::Await {
            value: ctx_ref.heap.extract(fut),
            dest_reg: dest_reg as u16,
        });
        ctx_ref.jit_panic_suspend_resume_ip = Some(resume_ip);

        // Outermost buffer, not this frame's — see `execute_jit_frame`.
        let buf = ctx_ref.jit_suspend_buf;
        if !buf.is_null() {
            crate::exec::ctx::my_longjmp(buf, 2);
        } else {
            panic!("JIT suspend triggered but no jump buffer registered");
        }
    }
}

pub(crate) extern "C" fn jit_yield(
    ctx: *mut ExecCtx,
    val: VmValue,
    dest_reg: u32,
    resume_ip: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        ctx_ref.frames[frame_idx].ip = resume_ip;
        ctx_ref.vm_suspend = Some(crate::exec::VmSuspend::Yield {
            value: val,
            dest_reg: dest_reg as u8,
        });
        ctx_ref.jit_panic_suspend_resume_ip = Some(resume_ip);

        // Outermost buffer, not this frame's — see `execute_jit_frame`.
        let buf = ctx_ref.jit_suspend_buf;
        if !buf.is_null() {
            crate::exec::ctx::my_longjmp(buf, 2);
        } else {
            panic!("JIT suspend triggered but no jump buffer registered");
        }
    }
}
