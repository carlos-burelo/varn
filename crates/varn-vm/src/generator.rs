use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use varn_types::generator::GeneratorDriver;

use crate::exec::{ExecCtx, VmSuspend};
use crate::value::VmValue;

fn make_iter_result(heap: &mut crate::heap::Heap, value: VmValue, done: bool) -> VmValue {
    let obj = varn_types::value::ObjRef::from_pairs([
        (Arc::from("value"), value),
        (Arc::from("done"), VmValue::from_bool(done)),
    ]);
    VmValue::from_heap_idx(heap.alloc(crate::heap::HeapObj::Object(obj)))
}

struct NanGenInner {
    ctx: Box<ExecCtx>,
    started: bool,
    done: bool,
    resume_dest: Option<u8>,
}

impl std::fmt::Debug for NanGenInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NanGenInner(done={})", self.done)
    }
}

/// Drives a generator body: runs it until it suspends, hands back
/// `{ value, done }`, and resumes it on the next call.
///
/// One driver serves both `function*` and `async function*`. They differ in a
/// single arm: what happens when the body suspends on `await`. A sync
/// generator has nothing to suspend into and says so; an async one settles the
/// awaited value and keeps running until the body reaches a `yield` or
/// finishes. Everything else — resume protocol, done bookkeeping, and the four
/// GC tracing walks — is identical, so it is written once.
#[derive(Debug)]
pub struct NanGenDriver {
    inner: RefCell<NanGenInner>,
    /// `async function*`: `await` inside the body is settled here rather than
    /// rejected.
    is_async: bool,
}

impl NanGenDriver {
    pub(crate) fn new(ctx: Box<ExecCtx>, is_async: bool) -> Rc<Self> {
        Rc::new(NanGenDriver {
            inner: RefCell::new(NanGenInner {
                ctx,
                started: false,
                done: false,
                resume_dest: None,
            }),
            is_async,
        })
    }
}

impl GeneratorDriver for NanGenDriver {
    fn next(&self, input: VmValue) -> Result<VmValue, String> {
        let mut inner = self.inner.borrow_mut();

        if inner.done {
            return Ok(make_iter_result(&mut inner.ctx.heap, VmValue::null(), true));
        }

        if inner.started {
            if let Some(dest_reg) = inner.resume_dest.take() {
                let input_nv = input;
                if let Some(frame) = inner.ctx.frames.last() {
                    let base = frame.base;
                    let nregs = frame.closure().proto.register_count as usize;
                    inner.ctx.stack.ensure_frame_size(base, nregs);
                    if (dest_reg as usize) < nregs {
                        // No puede fallar en programas bien tipados; si el
                        // input no encaja, el error sale como fallo del `next`.
                        let _ = inner
                            .ctx
                            .stack
                            .unbox_into_reg(base, dest_reg as usize, input_nv);
                    }
                }
            }
        }
        inner.started = true;

        // One `next()` can suspend several times: every `await` on the way to
        // the next `yield` comes back through here.
        loop {
            let result = inner.ctx.run_until(0);

            match inner.ctx.vm_suspend.take() {
                Some(VmSuspend::Yield {
                    value: nv,
                    dest_reg,
                }) => {
                    inner.resume_dest = Some(dest_reg);
                    return Ok(make_iter_result(&mut inner.ctx.heap, nv, false));
                }
                Some(VmSuspend::Await { value, dest_reg }) if self.is_async => {
                    match inner.ctx.settle_awaited(value) {
                        Ok(resolved) => inner.ctx.resume_with_awaited(dest_reg, resolved),
                        Err(thrown) => match inner.ctx.reject_awaited(thrown) {
                            crate::exec::ctx_tasks::Rejection::Caught => {}
                            crate::exec::ctx_tasks::Rejection::Unhandled(thrown) => {
                                inner.done = true;
                                return Err(format!("{thrown}"));
                            }
                        },
                    }
                }
                Some(VmSuspend::Await { .. }) => {
                    inner.done = true;
                    return Err(
                        "cannot use `await` inside a sync generator — declare it `async function*`"
                            .to_string(),
                    );
                }
                None => {
                    inner.done = true;
                    let ret = result.map_err(|e| e.message)?;
                    return Ok(make_iter_result(&mut inner.ctx.heap, ret, true));
                }
            }
        }
    }

    fn is_done(&self) -> bool {
        self.inner.borrow_mut().done
    }

    fn is_async(&self) -> bool {
        self.is_async
    }

    fn trace_vm_values(&self, callback: &mut dyn FnMut(varn_types::VmValue)) {
        let inner = self.inner.borrow();

        // Solo DYN y REF son raíces: GPR/FPR nunca alojan heap por
        // construcción y el colector ni los mira.
        for &nv in &inner.ctx.stack.dyn_ {
            callback(nv);
        }
        for &h in &inner.ctx.stack.refs {
            if h != crate::frame_store::REF_UNINIT {
                callback(VmValue::from_heap_idx(h));
            }
        }

        for frame in &inner.ctx.frames {
            for &c in frame.closure().constants.iter() {
                callback(c);
            }
            for uv in &frame.closure().upvalues {
                if let Ok(upval_inner) = uv.inner.try_borrow() {
                    callback(upval_inner.value);
                }
            }
        }

        for (_, uv) in &inner.ctx.open_upvalues {
            if let Ok(upval_inner) = uv.inner.try_borrow() {
                callback(upval_inner.value);
            }
        }

        for (_, nv) in &inner.ctx.pending_constructors {
            callback(*nv);
        }
        for (_, nv) in &inner.ctx.pending_setters {
            callback(*nv);
        }
    }

    fn trace_closures(&self, callback: &mut dyn FnMut(usize)) {
        let inner = self.inner.borrow();
        for frame in &inner.ctx.frames {
            callback(frame.closure_ptr as *const () as usize);
        }
    }
}
