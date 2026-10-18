use super::jit_frame;
use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(super) enum Entry {
    Interpret,
    NextFrame,
    Done(VmResult<VmValue>),
}

#[inline(always)]
#[allow(dangerous_implicit_autorefs)]
pub(super) unsafe fn enter(
    ctx: *mut ExecCtx,
    closure_ptr: *const VmClosure,
    closure: &VmClosure,
    frame_idx: usize,
    depth: usize,
) -> Entry {
    let is_first_entry = unsafe { (*ctx).frames[frame_idx].ip == 0 };

    let pending_osr = unsafe { (*ctx).osr_request.take() }.or_else(|| {
        let ip = unsafe { (*ctx).frames[frame_idx].ip };
        let proto = &closure.proto;
        (!is_first_entry
            && (proto.is_async || proto.is_generator)
            && proto.ssa.get().is_some_and(|ssa| ssa.is_resume_entry(ip)))
        .then_some(varn_types::ssa::OsrRequest {
            ip,
            mode: varn_types::ssa::OsrMode::Resume,
        })
    });
    let osr_fn = match pending_osr {
        Some(req) if unsafe { req.ip == (*ctx).frames[frame_idx].ip } => {
            let jit_on = unsafe { !(*ctx).settings.no_jit };
            let f = jit_on.then(|| closure.osr_jit_fn(req)).flatten();
            match (f, req.mode) {
                (Some(f), _) => Some(f),
                (None, varn_types::ssa::OsrMode::Resume) => unsafe {
                    let ctxm = &mut *ctx;
                    if ctxm.jit_error_flag != 0 {
                        ctxm.jit_error_flag = 0;
                        let thrown = ctxm.jit_native_result;
                        match crate::exec::exceptions::raise(ctx, thrown, depth) {
                            Ok(()) => return Entry::NextFrame,
                            Err(err) => return Entry::Done(Err(err)),
                        }
                    } else {
                        None
                    }
                },
            }
        }
        Some(_) | None => None,
    };
    let is_osr = osr_fn.is_some();

    let hot_fn = match osr_fn {
        Some(f) => Some(f),
        None if unsafe { !(*ctx).settings.no_jit } && is_first_entry => closure.hot_jit_fn(),
        None => None,
    };
    if let Some(jit_fn) = hot_fn {
        let outcome = unsafe {
            jit_frame::run_compiled_frame(
                ctx,
                jit_fn,
                closure_ptr,
                closure,
                frame_idx,
                depth,
                is_first_entry,
                is_osr,
            )
        };
        return match outcome.into_result() {
            None => Entry::NextFrame,
            Some(result) => Entry::Done(result),
        };
    } else if is_first_entry {
        varn_jit::JIT_STATS
            .interp_runs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    Entry::Interpret
}
