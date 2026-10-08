use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use varn_types::generator::GeneratorDriver;
use varn_types::{NativeCtx, NativeFnResult};

use crate::exec::{ExecCtx, VmSuspend};
use crate::value::VmValue;

pub(crate) fn generator_next(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let gen_nv = args
        .first()
        .copied()
        .ok_or("generator.next: missing receiver")?;
    let gen = ctx
        .as_generator(gen_nv)
        .ok_or("generator.next: invalid receiver")?;
    let input = args.get(1).copied().unwrap_or(VmValue::null());
    gen.0.next(input).map_err(Into::into)
}

fn make_iter_result(heap: &mut crate::heap::Heap, value: VmValue, done: bool) -> VmValue {
    heap.alloc_object_pairs([
        (Arc::from("value"), value),
        (Arc::from("done"), VmValue::from_bool(done)),
    ])
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

#[derive(Debug)]
pub struct NanGenDriver {
    inner: RefCell<NanGenInner>,

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
                        let _ = inner
                            .ctx
                            .stack
                            .unbox_into_reg(base, dest_reg as usize, input_nv);
                    }
                }
            }
        }
        inner.started = true;

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
                Some(VmSuspend::DebugBreak) => {}
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

        for &nv in &inner.ctx.stack.dyn_ {
            callback(nv);
        }
        for &h in inner.ctx.stack.refs.iter().flatten() {
            callback(VmValue::from_heap(h));
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
