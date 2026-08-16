use std::cell::RefCell;
use std::rc::Rc;
use varn_types::generator::GeneratorDriver;
use varn_types::value::Value;

use crate::exec::{ExecCtx, VmSuspend};
use crate::value::VmValue;

/// `{ value, done }`, built straight from the yielded slot.
///
/// The yielded value stays a `VmValue` all the way through on purpose. This
/// used to `heap.extract` it into a `Value` and then push it back through
/// `value_to_nv`, which cannot represent anything heap-allocated without a
/// heap to intern into — it has a `debug_assert!(false, "must be pre-interned")`
/// for exactly that and returns null in release. So `yield [1, 2]` and
/// `yield { a: 1 }` came out as `null`, while `yield 7` and short strings
/// survived because they fit inline. The generator shares its caller's heap
/// (`Heap` is an `Rc<UnsafeCell<…>>` handle), so the index is valid on both
/// sides and the round trip bought nothing.
fn make_iter_result(value: VmValue, done: bool) -> Value {
    varn_types::value::new_object(varn_types::value::ObjRef::from_pairs([
        (Rc::from("value"), value),
        (Rc::from("done"), varn_types::VmValue::from_bool(done)),
    ]))
}

struct NanSyncGenInner {
    ctx: Box<ExecCtx>,
    started: bool,
    done: bool,
    resume_dest: Option<u8>,
}

impl std::fmt::Debug for NanSyncGenInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NanSyncGenInner(done={})", self.done)
    }
}

#[derive(Debug)]
pub struct NanSyncGenDriver {
    inner: RefCell<NanSyncGenInner>,
}

impl NanSyncGenDriver {
    pub(crate) fn new(ctx: Box<ExecCtx>) -> Rc<Self> {
        Rc::new(NanSyncGenDriver {
            inner: RefCell::new(NanSyncGenInner {
                ctx,
                started: false,
                done: false,
                resume_dest: None,
            }),
        })
    }
}

impl GeneratorDriver for NanSyncGenDriver {
    fn next(&self, input: Value) -> Result<Value, String> {
        let mut inner = self.inner.borrow_mut();

        if inner.done {
            return Ok(make_iter_result(VmValue::null(), true));
        }

        if inner.started {
            if let Some(dest_reg) = inner.resume_dest.take() {
                let input_nv = inner.ctx.heap.intern(input);
                if let Some(frame) = inner.ctx.frames.last() {
                    let slot = frame.base + dest_reg as usize;
                    if slot < inner.ctx.stack.len() {
                        inner.ctx.stack[slot] = input_nv;
                    } else {
                        inner.ctx.stack.resize(slot + 1, VmValue::null());
                        inner.ctx.stack[slot] = input_nv;
                    }
                }
            }
        }
        inner.started = true;

        let result = inner.ctx.run_until(0);

        match inner.ctx.vm_suspend.take() {
            Some(VmSuspend::Yield {
                value: nv,
                dest_reg,
            }) => {
                inner.resume_dest = Some(dest_reg);
                Ok(make_iter_result(nv, false))
            }
            Some(VmSuspend::Task(_)) | Some(VmSuspend::Await { .. }) => {
                inner.done = true;
                Err("cannot use `await` inside a sync generator (`function*`)".to_string())
            }
            None => {
                inner.done = true;
                let ret = result.map_err(|e| e.message)?;
                Ok(make_iter_result(ret, true))
            }
        }
    }

    fn is_done(&self) -> bool {
        self.inner.borrow_mut().done
    }

    fn is_async(&self) -> bool {
        false
    }

    fn trace_vm_values(&self, callback: &mut dyn FnMut(varn_types::VmValue)) {
        let inner = self.inner.borrow();

        for &nv in &inner.ctx.stack {
            callback(varn_types::VmValue(nv.0));
        }

        for frame in &inner.ctx.frames {
            for &c in frame.closure().constants.iter() {
                callback(varn_types::VmValue(c.0));
            }
            for uv in &frame.closure().upvalues {
                if let Ok(upval_inner) = uv.inner.try_borrow() {
                    callback(varn_types::VmValue(upval_inner.value.0));
                }
            }
        }

        for (_, uv) in &inner.ctx.open_upvalues {
            if let Ok(upval_inner) = uv.inner.try_borrow() {
                callback(varn_types::VmValue(upval_inner.value.0));
            }
        }

        for (_, nv) in &inner.ctx.pending_constructors {
            callback(varn_types::VmValue(nv.0));
        }
        for (_, nv) in &inner.ctx.pending_setters {
            callback(varn_types::VmValue(nv.0));
        }
    }

    fn trace_vm_values_mut(&self, callback: &mut dyn FnMut(&mut varn_types::VmValue)) {
        let mut inner = self.inner.borrow_mut();
        let ctx = &mut *inner.ctx;

        for nv in ctx.stack.iter_mut() {
            callback(nv);
        }

        // Closure constants are interned (old gen) and never hold nursery
        // indices; upvalues can.
        for frame in &ctx.frames {
            for uv in &frame.closure().upvalues {
                if let Ok(mut upval_inner) = uv.inner.try_borrow_mut() {
                    callback(&mut upval_inner.value);
                }
            }
        }
        for (_, uv) in &ctx.open_upvalues {
            if let Ok(mut upval_inner) = uv.inner.try_borrow_mut() {
                callback(&mut upval_inner.value);
            }
        }

        for (_, nv) in ctx.pending_constructors.iter_mut() {
            callback(nv);
        }
        for (_, nv) in ctx.pending_setters.iter_mut() {
            callback(nv);
        }
        if let Some(VmSuspend::Yield { value, .. }) = &mut ctx.vm_suspend {
            callback(value);
        }
    }

    fn trace_closures(&self, callback: &mut dyn FnMut(usize)) {
        let inner = self.inner.borrow();
        for frame in &inner.ctx.frames {
            callback(frame.closure_ptr as *const () as usize);
        }
    }
}
