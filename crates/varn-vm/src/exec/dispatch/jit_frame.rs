use super::ExecCtx;
use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::frame_ctrl::{resolve_constructor_return, unwind_to_handler};
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
    let saved = unsafe { (*ctx).jit_jmp_buf };
    let is_outer = saved.is_null();
    let exits_len = unsafe { (*ctx).jit_exits_saved.len() };
    unsafe { (*ctx).jit_exits_saved.push((*ctx).jit_exit) };
    unsafe { (*ctx).jit_exit = varn_jit::stack_roots::JitExit::default() };
    let mut jmp_buf = crate::exec::ctx::JmpBuf::default();
    let jmp_res = std::hint::black_box(unsafe { crate::exec::ctx::my_setjmp(&mut jmp_buf) });

    if jmp_res == 0 {
        unsafe { (*ctx).jit_jmp_buf = &mut jmp_buf as *mut crate::exec::ctx::JmpBuf };
        if is_outer {
            unsafe { (*ctx).jit_suspend_buf = (*ctx).jit_jmp_buf };
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
        unsafe { (*ctx).jit_jmp_buf = saved };
        if is_outer {
            unsafe { (*ctx).jit_suspend_buf = std::ptr::null_mut() };
        }
        Ok(val)
    } else {
        unsafe { restore_exit(ctx, exits_len) };
        unsafe { (*ctx).jit_jmp_buf = saved };
        if is_outer {
            unsafe { (*ctx).jit_suspend_buf = std::ptr::null_mut() };
        }
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
    }
    if unsafe { (*ctx).settings.trace } {
        let what = if is_osr { "JIT OSR" } else { "JIT ENTRY" };
        let at = unsafe { (*ctx).frames[frame_idx].ip };
        unsafe { (*ctx).trace_event(what, frame_idx, closure, at, None) };
    }

    let base = unsafe { (*ctx).frames[frame_idx].base };
    let res = unsafe { execute_jit_frame(ctx, jit_fn, closure_ptr, base) };

    let res = match res {
        Ok(val) => val,
        Err(code) => {
            if code == 1 {
                let handler = unsafe { (*ctx).try_handlers.pop_if(|h| h.frame_depth > depth) };
                let error = unsafe {
                    (*ctx)
                        .jit_panic_exception_error
                        .take()
                        .unwrap_or(VmValue::null())
                };
                let err_obj = unsafe { (*ctx).jit_panic_exception_err_obj.take() };

                if let Some(handler) = handler {
                    if let Some(catch_fn) = unsafe {
                        catch_entry_fn(ctx, closure, frame_idx, &handler)
                    } {
                        unsafe { (*ctx).jit_native_result = error };
                        return unsafe {
                            run_compiled_frame(
                                ctx,
                                catch_fn,
                                closure_ptr,
                                closure,
                                frame_idx,
                                depth,
                                false,
                                true,
                            )
                        };
                    }
                    if let Err(e) = unwind_to_handler(unsafe { &mut *ctx }, handler, error) {
                        return JitFrameOutcome::Failed(e);
                    }
                    return JitFrameOutcome::Continue;
                } else {
                    return JitFrameOutcome::Failed(err_obj.unwrap());
                }
            } else if code == 2 {
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
unsafe fn catch_entry_fn(
    ctx: *mut ExecCtx,
    closure: &VmClosure,
    frame_idx: usize,
    handler: &crate::frame::TryHandler,
) -> Option<varn_jit::JitFn> {
    let ctxm = &mut *ctx;
    if ctxm.settings.no_jit {
        return None;
    }
    if ctxm.frames.len() != frame_idx + 1 || handler.frame_depth != ctxm.frames.len() {
        return None;
    }
    let ssa = closure.proto.ssa.get()?;
    ssa.catch_at(handler.catch_ip)?;
    let req = varn_types::ssa::OsrRequest {
        ip: handler.catch_ip,
        mode: varn_types::ssa::OsrMode::Catch,
    };
    closure.osr_jit_fn(req)
}

#[allow(dangerous_implicit_autorefs)]
unsafe fn finish_compiled_frame(
    ctx: *mut ExecCtx,
    depth: usize,
    res: VmValue,
) -> JitFrameOutcome {
    let returning_frame_idx = unsafe { (*ctx).frames.len() }.saturating_sub(1);
    let frame = unsafe { (*ctx).frames.pop() }.unwrap();
    unsafe { (*ctx).drop_frame_storage(frame.base) };
    let is_module_frame = frame.closure().proto.name.as_deref() == Some("<module>")
        && !frame.closure().proto.chunk.source_file.is_empty();

    let final_val = resolve_constructor_return(unsafe { &mut *ctx }, returning_frame_idx, res);

    if is_module_frame {
        let source_file = frame.closure().proto.chunk.source_file.to_string();
        let module_exports = unsafe { (*ctx).module_exports.remove(&returning_frame_idx) };
        let cached = module_exports.unwrap_or(final_val);
        let module_id = varn_core::ModuleId::from_canonical_str(&source_file);
        unsafe { &mut *(*ctx).modules.get() }.insert(module_id, cached);
    }

    if frame.return_reg != crate::frame::CallFrame::NO_RETURN_REG {
        if let Some(caller) = unsafe { (*ctx).frames.last() } {
            let caller_base = caller.base;
            if let Err(e) = unsafe {
                (*ctx)
                    .stack
                    .unbox_into_reg(caller_base, frame.return_reg as usize, final_val)
            } {
                return JitFrameOutcome::Failed(e);
            }
        }
    }

    if unsafe { (*ctx).frames.len() } == depth {
        JitFrameOutcome::Done(final_val)
    } else {
        JitFrameOutcome::Continue
    }
}
