use super::ExecCtx;
use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;

pub(super) enum JitFrameOutcome {
    Continue,

    Done(VmValue),

    Failed(RuntimeError),
}

impl JitFrameOutcome {
    #[inline(always)]
    pub(super) fn into_result(self) -> Option<VmResult<VmValue>> {
        match self {
            JitFrameOutcome::Continue => None,
            JitFrameOutcome::Done(v) => Some(Ok(v)),
            JitFrameOutcome::Failed(e) => Some(Err(e)),
        }
    }
}

#[inline(never)]
unsafe fn execute_jit_frame(
    ctx: *mut ExecCtx,
    jit_fn: varn_jit::JitFn,
    closure_ptr: *const VmClosure,
    base: usize,
) -> Result<VmValue, i32> {
    let exits_len = unsafe { (*ctx).jit_exits_saved.len() };
    let frameless_depth = unsafe { (*ctx).jit_frameless_depth };
    unsafe { (*ctx).jit_exits_saved.push((*ctx).jit_exit) };
    unsafe { (*ctx).jit_exit = varn_jit::stack_roots::JitExit::default() };
    let mut jmp_buf = crate::exec::ctx::JmpBuf::default();
    let jmp_res = std::hint::black_box(unsafe { crate::exec::ctx::my_setjmp(&mut jmp_buf) });

    if jmp_res == 0 {
        let saved_suspend = unsafe { (*ctx).jit_suspend_buf };
        let is_outer = saved_suspend.is_null();
        if is_outer {
            unsafe { (*ctx).jit_suspend_buf = &mut jmp_buf as *mut crate::exec::ctx::JmpBuf };
        }
        let val = unsafe {
            (jit_fn)(
                std::ptr::null_mut(),
                closure_ptr as *const std::ffi::c_void,
                base,
                ctx as *mut std::ffi::c_void,
            )
        };
        std::hint::black_box(ctx);
        unsafe { restore_exit(ctx, exits_len) };
        unsafe { (*ctx).jit_frameless_depth = frameless_depth };
        if is_outer {
            unsafe { (*ctx).jit_suspend_buf = saved_suspend };
        }
        Ok(val)
    } else {
        unsafe { restore_exit(ctx, exits_len) };
        unsafe { (*ctx).jit_frameless_depth = frameless_depth };
        unsafe { (*ctx).jit_suspend_buf = std::ptr::null_mut() };
        Err(jmp_res)
    }
}

unsafe fn restore_exit(ctx: *mut ExecCtx, exits_len: usize) {
    let ctx = unsafe { &mut *ctx };
    ctx.jit_exit = ctx.jit_exits_saved[exits_len];
    ctx.jit_exits_saved.truncate(exits_len);
}
#[allow(dangerous_implicit_autorefs)]
pub(super) unsafe fn run_compiled_frame(
    ctx: *mut ExecCtx,
    jit_fn: varn_jit::JitFn,
    closure_ptr: *const VmClosure,
    closure: &VmClosure,
    frame_idx: usize,
    depth: usize,
    is_first_entry: bool,
    is_osr: bool,
) -> JitFrameOutcome {
    if is_first_entry {
        varn_jit::JIT_STATS
            .jit_runs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if is_osr {
        varn_jit::JIT_STATS
            .osr_entries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let owner = frame_idx + 1;
        unsafe { (*ctx).try_handlers.retain(|h| h.frame_depth != owner) };
    }
    if unsafe { (*ctx).settings.trace } {
        let what = if is_osr { "JIT OSR" } else { "JIT ENTRY" };
        let at = unsafe { (*ctx).frames[frame_idx].ip };
        unsafe { (*ctx).trace_event(what, frame_idx, closure, at, None) };
    }

    let base = unsafe { (*ctx).frames[frame_idx].base };
    if unsafe { (*ctx).jit_error_flag } == 0 {
        unsafe { (*ctx).jit_panic_exception_error = None };
        unsafe { (*ctx).jit_panic_exception_err_obj = None };
    }
    let res = unsafe { execute_jit_frame(ctx, jit_fn, closure_ptr, base) };

    let res = match res {
        Ok(_)
            if matches!(
                unsafe { &(*ctx).vm_suspend },
                Some(crate::exec::VmSuspend::Yield { .. })
            ) =>
        {
            return JitFrameOutcome::Done(VmValue::null());
        }
        Ok(val) => {
            if unsafe { (*ctx).jit_error_flag } != 0 {
                unsafe { (*ctx).jit_error_flag = 0 };
                let err_obj = unsafe { (*ctx).jit_panic_exception_err_obj.take() };
                let error_val = unsafe {
                    (*ctx)
                        .jit_panic_exception_error
                        .take()
                        .unwrap_or((*ctx).jit_native_result)
                };
                if unsafe { (*ctx).frames.len() } > depth {
                    let frame = unsafe { (*ctx).frames.pop() }.unwrap();
                    unsafe { (*ctx).drop_frame_storage(frame.base) };
                    unsafe {
                        (*ctx)
                            .try_handlers
                            .retain(|h| h.frame_depth <= (*ctx).frames.len());
                    }
                    if frame.awaited && unsafe { (*ctx).frames.len() } > depth {
                        unsafe { (*ctx).reject_awaiter(error_val) };
                        return JitFrameOutcome::Continue;
                    }
                }
                if unsafe { crate::exec::exceptions::dispatch_to_handler(ctx, error_val, depth) } {
                    return JitFrameOutcome::Continue;
                }
                if let Some(e) = err_obj {
                    return JitFrameOutcome::Failed(e);
                }
                let e = unsafe {
                    crate::exec::exceptions::build_thrown_error(
                        error_val,
                        &(*ctx).heap,
                        &(*ctx).frames,
                    )
                };
                return JitFrameOutcome::Failed(e);
            }
            val
        }
        Err(code) => {
            if code == 2 {
                unsafe { (*ctx).jit_error_flag = 0 };
                if let Some(resume_ip) = unsafe { (*ctx).jit_panic_suspend_resume_ip.take() } {
                    let frame_idx2 = unsafe { (*ctx).frames.len() } - 1;
                    unsafe { (*ctx).frames[frame_idx2].ip = resume_ip };
                }
                return JitFrameOutcome::Done(VmValue::null());
            } else {
                panic!("Unknown longjmp code: {}", code);
            }
        }
    };

    unsafe { finish_compiled_frame(ctx, depth, res) }
}

#[allow(dangerous_implicit_autorefs)]
unsafe fn finish_compiled_frame(ctx: *mut ExecCtx, depth: usize, res: VmValue) -> JitFrameOutcome {
    let final_val = match unsafe { (*ctx).complete_return(res) } {
        Ok(value) => value,
        Err(err) => {
            return match unsafe { crate::exec::exceptions::handle_runtime_error(ctx, err, depth) } {
                Ok(()) => JitFrameOutcome::Continue,
                Err(err) => JitFrameOutcome::Failed(err),
            }
        }
    };

    if unsafe { (*ctx).frames.len() } == depth {
        JitFrameOutcome::Done(final_val)
    } else {
        JitFrameOutcome::Continue
    }
}
