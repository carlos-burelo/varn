use std::cell::RefCell;
use std::rc::Rc;
use varn_types::generator::{GenChannel, GeneratorDriver};
use varn_types::value::Value;

use crate::exec::{ExecCtx, VmSuspend};
use crate::value::VmValue;

fn make_iter_result(value: Value, done: bool) -> Value {
    let mut obj = varn_types::value::ObjData::new();
    obj.set_field(Rc::from("value"), value);
    obj.set_field(Rc::from("done"), Value::Bool(done));
    varn_types::value::new_object(obj)
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
    pub fn new(ctx: Box<ExecCtx>) -> Rc<Self> {
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
            return Ok(make_iter_result(Value::Null, true));
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
                let val = inner.ctx.heap.extract(nv);
                Ok(make_iter_result(val, false))
            }
            Some(VmSuspend::Task(_)) | Some(VmSuspend::Await { .. }) => {
                inner.done = true;
                Err("cannot use `await` inside a sync generator (`function*`)".to_string())
            }
            None => {
                inner.done = true;
                let ret = result.map_err(|e| e.message)?;
                let val = inner.ctx.heap.extract(ret);
                Ok(make_iter_result(val, true))
            }
        }
    }

    fn is_done(&self) -> bool {
        self.inner.borrow_mut().done
    }

    fn is_async(&self) -> bool {
        false
    }
}
