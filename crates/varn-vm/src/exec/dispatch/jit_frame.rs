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
use crate::exec::frame_ctrl::resolve_constructor_return;
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

/// Enter `jit_fn` for the frame at `frame_idx` and reconcile whatever comes
/// back.
///
/// # Safety
///
/// `ctx` and `closure_ptr` must be valid, and `frame_idx` must be the index of
/// the top frame — the caller reads it back after the compiled code has had a
/// chance to push and pop frames of its own.
#[allow(clippy::too_many_arguments)]
// Same suppression `run_until_inner_raw` carries, for the same reason: this
// code reaches `ExecCtx` fields through `*ctx` throughout, and an autoref
// there is exactly what it means to.
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
    let res = ExecCtx::execute_jit_frame(ctx, jit_fn, closure_ptr, base);

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
                    while (*ctx).frames.len() > handler.frame_depth {
                        (*ctx).record_frame_pop();
                        let f = (*ctx).frames.pop().unwrap();
                        (*ctx).close_upvalues_above(f.base);
                    }

                    let f2 = (*ctx).frames.len() - 1;
                    let b2 = (*ctx).frames[f2].base;
                    let required_depth =
                        b2 + (*ctx).frames[f2].closure().proto.register_count as usize;
                    (*ctx).stack.truncate(required_depth);
                    let thrown_val = error;

                    let slot = b2 + handler.err_reg as usize;
                    if slot < (*ctx).stack.len() {
                        (*ctx).stack[slot] = thrown_val;
                    } else {
                        (*ctx).stack.resize(slot + 1, VmValue::null());
                        (*ctx).stack[slot] = thrown_val;
                    }
                    let new_frame_idx = (*ctx).frames.len() - 1;
                    (*ctx).frames[new_frame_idx].ip = handler.catch_ip;
                    return JitFrameOutcome::Continue;
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
    if (*ctx).settings.trace {
        (*ctx).trace_event("JIT EXIT", frame_idx, closure, 0, None);
    }

    let frame = (*ctx).frames.pop().unwrap();
    (*ctx).record_frame_pop();
    while (*ctx)
        .try_handlers
        .last()
        .map(|h| h.frame_depth > (*ctx).frames.len())
        .unwrap_or(false)
    {
        (*ctx).try_handlers.pop();
    }
    (*ctx).close_upvalues_above(frame.base);
    (*ctx).stack.truncate(frame.base);

    let final_val = resolve_constructor_return(&mut *ctx, frame_idx, res);

    if let Some(return_reg) = frame.return_reg {
        let caller_base = (*ctx).frames.last().map(|f| f.base).unwrap_or(0);
        (*ctx).stack[caller_base + return_reg as usize] = final_val;
    }

    if (*ctx).frames.len() == depth {
        return JitFrameOutcome::Done(final_val);
    }
    JitFrameOutcome::Continue
}

/// Convenience for the caller: turn an outcome into the loop's own control
/// flow is not possible across a function boundary, so this is only the
/// `VmResult` half of it.
impl JitFrameOutcome {
    pub(super) fn into_result(self) -> Option<VmResult<VmValue>> {
        match self {
            JitFrameOutcome::Continue => None,
            JitFrameOutcome::Done(v) => Some(Ok(v)),
            JitFrameOutcome::Failed(e) => Some(Err(e)),
        }
    }
}
