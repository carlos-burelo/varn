//! Running one frame as COMPILED code, and everything that can come back out
//! of it.
//!
//! Extracted from `run_until_inner_raw` because it is a different job from
//! interpreting bytecode: the interpreter loop steps opcodes, this steps a
//! whole activation and then has to reconcile four possible endings —
//! returned normally, threw and was caught below, threw and was not, or
//! suspended. That reconciliation is ~120 lines that ran once per frame entry
//! and had nothing to do with the per-opcode dispatch it was wedged inside.
//!
//! It cannot simply be a function that returns a value: the original code
//! reached `continue 'frame_loop` and two different `return`s from inside a
//! labelled loop, and control flow like that does not cross a function
//! boundary. [`JitFrameOutcome`] carries the decision back out instead, so the
//! caller performs the jump and this module stays honest about the fact that
//! there are four endings, not one.

use super::ExecCtx;
use crate::closure::VmClosure;
use crate::error::{RuntimeError, VmResult};
use crate::exec::frame_ctrl::{resolve_constructor_return, unwind_to_handler};
use crate::value::VmValue;

/// How a compiled frame ended, as an instruction to the frame loop.
pub(super) enum JitFrameOutcome {
    /// Go round the frame loop again. Either the frame returned and a caller
    /// is still running, or an exception was caught and its handler's frame is
    /// now on top with its ip already set to the catch block.
    Continue,
    /// The run this call started is over; this is its value.
    Done(VmValue),
    /// Nothing below caught it.
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

/// Run one clif frame under its OWN jump buffer.
#[inline(never)]
unsafe fn execute_jit_frame(
    ctx: *mut ExecCtx,
    jit_fn: varn_jit::JitFn,
    closure_ptr: *const VmClosure,
    base: usize,
) -> Result<VmValue, i32> {
    let saved = (*ctx).jit_jmp_buf;
    let is_outer = saved.is_null();
    let mut jmp_buf = crate::exec::ctx::JmpBuf::default();
    let jmp_res = std::hint::black_box(crate::exec::ctx::my_setjmp(&mut jmp_buf));

    if jmp_res == 0 {
        (*ctx).jit_jmp_buf = &mut jmp_buf as *mut crate::exec::ctx::JmpBuf;
        if is_outer {
            (*ctx).jit_suspend_buf = (*ctx).jit_jmp_buf;
        }
        (*ctx).jit_frame_prepushed = 1;
        let required = base + (&(*closure_ptr).proto).register_count as usize;
        if (*ctx).stack.len() < required {
            (*ctx).stack.resize(required, VmValue::null());
        }
        let val = (jit_fn)(
            (*ctx).stack.as_mut_ptr() as *mut std::ffi::c_void,
            closure_ptr as *const std::ffi::c_void,
            base,
            ctx as *mut std::ffi::c_void,
        );
        std::hint::black_box(ctx);
        (*ctx).jit_jmp_buf = saved;
        if is_outer {
            (*ctx).jit_suspend_buf = std::ptr::null_mut();
        }
        Ok(val)
    } else {
        (*ctx).jit_jmp_buf = saved;
        if is_outer {
            (*ctx).jit_suspend_buf = std::ptr::null_mut();
        }
        Err(jmp_res)
    }
}

/// Enter `jit_fn` for the frame at `frame_idx` and reconcile whatever comes
/// back.
///
/// # Safety
///
/// `ctx` and `closure_ptr` must be valid, and `frame_idx` must be the index of
/// the top frame — the caller reads it back after the compiled code has had a
/// chance to push and pop frames of its own.
#[allow(clippy::too_many_arguments)]
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
        // Not counted in `jit_runs`: this frame already counted as an
        // interpreted entry when it started, and it is the same frame.
        // `osr_entries` is what says the rescue happened.
        varn_jit::JIT_STATS
            .osr_entries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if (*ctx).settings.trace {
        let what = if is_osr { "JIT OSR" } else { "JIT ENTRY" };
        let at = (*ctx).frames[frame_idx].ip;
        (*ctx).trace_event(what, frame_idx, closure, at, None);
    }

    let base = (*ctx).frames[frame_idx].base;
    let res = execute_jit_frame(ctx, jit_fn, closure_ptr, base);

    let res = match res {
        Ok(val) => val,
        Err(code) => {
            if code == 1 {
                let handler = (*ctx).jit_panic_exception_handler.take();
                let error = (*ctx)
                    .jit_panic_exception_error
                    .take()
                    .unwrap_or(VmValue::null());
                let err_obj = (*ctx).jit_panic_exception_err_obj.take();

                if let Some(handler) = handler {
                    if handler.frame_depth > depth {
                        unwind_to_handler(&mut *ctx, handler, error);
                        return JitFrameOutcome::Continue;
                    } else {
                        (*ctx).jit_panic_exception_handler = Some(handler);
                        (*ctx).jit_panic_exception_error = Some(error);
                        let err = err_obj.unwrap_or_else(|| {
                            crate::error::RuntimeError::new(format!("unhandled exception: {error}"))
                        });
                        (*ctx).jit_panic_exception_err_obj = Some(err.clone());
                        return JitFrameOutcome::Failed(err);
                    }
                } else {
                    return JitFrameOutcome::Failed(err_obj.unwrap());
                }
            } else if code == 2 {
                // Absent when the suspending helper parked a frame other than
                // the top one itself (see `jit_suspend_at`): a module that
                // suspends on a top-level await leaves its frame above ours.
                if let Some(resume_ip) = (*ctx).jit_panic_suspend_resume_ip.take() {
                    let frame_idx2 = (*ctx).frames.len() - 1;
                    (*ctx).frames[frame_idx2].ip = resume_ip;
                }
                return JitFrameOutcome::Done(VmValue::null());
            } else {
                panic!("Unknown longjmp code: {}", code);
            }
        }
    };

    // A compiled frame that executed a non-tail call pushed caller frames
    // beneath its own callee(s); the frame that just returned is the one
    // at the top of the stack NOW, which is not necessarily `frame_idx` if
    // a helper popped it itself. Read it live.
    let returning_frame_idx = (*ctx).frames.len().saturating_sub(1);
    let frame = (*ctx).frames.pop().unwrap();
    (*ctx).close_upvalues_above(frame.base);
    let is_module_frame = frame.closure().proto.name.as_deref() == Some("<module>")
        && !frame.closure().proto.chunk.source_file.is_empty();

    if let Some(caller) = (*ctx).frames.last() {
        let caller_req = caller.base + caller.closure().proto.register_count as usize;
        if (*ctx).stack.len() < caller_req {
            (*ctx).stack.resize(caller_req, VmValue::null());
        } else {
            (*ctx).stack.truncate(caller_req);
        }
    }

    let final_val = resolve_constructor_return(&mut *ctx, returning_frame_idx, res);

    if is_module_frame {
        let source_file = frame.closure().proto.chunk.source_file.to_string();
        let module_exports = (*ctx).module_exports.remove(&returning_frame_idx);
        let cached = module_exports.unwrap_or(final_val);
        let module_id = varn_core::ModuleId::from_canonical_str(&source_file);
        (*ctx).modules.insert(module_id, cached);
    }

    if let Some(return_reg) = frame.return_reg {
        let caller_base = (*ctx).frames.last().map(|f| f.base).unwrap_or(0);
        (*ctx).stack[caller_base + return_reg as usize] = final_val;
    }

    if (*ctx).frames.len() == depth {
        JitFrameOutcome::Done(final_val)
    } else {
        JitFrameOutcome::Continue
    }
}
