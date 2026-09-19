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

/// Enter `jit_fn` for the frame at `frame_idx` and reconcile whatever comes
/// back.
///
/// # Safety
///
/// `ctx` and `closure_ptr` must be valid, and `frame_idx` must be the index of
/// the top frame — the caller reads it back after the compiled code has had a
/// chance to push and pop frames of its own.
///
/// K3-faseA: inalcanzable — `FRAME_LAYOUT_V2_JIT_BAIL` impide compilar, así que
/// ningún `JitFn` llega aquí. El cuerpo original (setjmp, salida con 4
/// finales, `resolve_constructor_return`, unwind) se restaura de git en la
/// fase B junto con el lowering al layout por clases.
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
    let _ = (
        ctx, jit_fn, closure_ptr, closure, frame_idx, depth, is_first_entry, is_osr,
    );
    unreachable!("K3-faseA: compiled frames are bailed; see FRAME_LAYOUT_V2_JIT_BAIL");
}
