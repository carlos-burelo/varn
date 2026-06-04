use std::cell::RefCell;
use std::rc::Rc;

use crate::frame::{CallFrame, VmClosure, VmUpvalue};
use crate::value::VmValue;

use super::ctx::ExecCtx;
use super::VmSuspend;

impl ExecCtx {
    pub fn wait_task_handle(task: varn_types::AsyncTask) -> Result<varn_types::Value, String> {
        match task.peek_state() {
            varn_types::task::TaskState::Resolved(v) => return Ok(v),
            varn_types::task::TaskState::Rejected(v) => {
                return Err(format!("{v}"));
            }
            varn_types::task::TaskState::Pending => {}
        }

        let (tx, rx) = std::sync::mpsc::channel();
        task.on_settle(move |result| {
            let _ = tx.send(result);
        });

        match rx.recv() {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(v)) => Err(format!("{v}")),
            Err(_) => Err("task dropped".to_owned()),
        }
    }

    pub fn trace_event(
        &self,
        label: &str,
        frame_idx: usize,
        closure: &VmClosure,
        op_ip: usize,
        op: Option<varn_core::OpCode>,
    ) {
        if !self.trace {
            return;
        }

        let fn_name = closure.proto.name.as_deref().unwrap_or("<anon>");
        let line = closure.proto.chunk.lines.get_line(op_ip);
        varn_utilities::terminal::tagged(
            format_args!("vm:{label}"),
            format_args!("fn={fn_name} file={} frame={} ip={} line={} stack={} tries={} op={:?}",
                closure.proto.chunk.source_file, frame_idx, op_ip, line,
                self.stack.len(), self.try_handlers.len(), op,
            ),
        );
    }

    pub fn run_lazy_task_sync(
        &mut self,
        task: &varn_types::value::LazyTask,
    ) -> varn_types::AsyncTask {
        let mut fork = self.fork_for_task();
        let constants: Vec<VmValue> = task
            .closure
            .resolved_constants
            .iter()
            .map(|v| fork.heap.intern(v.clone()))
            .collect();
        let upvalues = task
            .closure
            .upvalues
            .iter()
            .map(|uv| {
                let val = uv.inner.borrow_mut().value.clone();
                VmUpvalue::closed(fork.heap.intern(val))
            })
            .collect();
        let closure = Rc::new(VmClosure::with_upvalues(
            Rc::clone(&task.closure.proto),
            upvalues,
            Rc::new(constants),
        ));
        let stack_values: Vec<VmValue> = task
            .args
            .iter()
            .cloned()
            .map(|value| fork.heap.intern(value))
            .collect();
        fork.stack.clear();
        fork.stack.extend(stack_values);
        let required = task.closure.proto.register_count as usize;
        if fork.stack.len() < required {
            fork.stack.resize(required, VmValue::null());
        }
        let mut frame = CallFrame::new(closure, 0);
        frame.current_class = task.current_class.clone();
        fork.frames.push(frame);

        let output = varn_types::AsyncTask::pending();
        loop {
            match fork.run() {
                Ok(result) => match fork.vm_suspend.take() {
                    Some(VmSuspend::Await { value, dest_reg }) => {
                        let res_val = match value {
                            varn_types::Value::Task(lazy) => {
                                let h = fork.run_lazy_task_sync(lazy.as_ref());
                                match h.peek_state() {
                                    varn_types::task::TaskState::Resolved(v) => Ok(v),
                                    varn_types::task::TaskState::Rejected(v) => Err(v),
                                    _ => Ok(varn_types::Value::Null),
                                }
                            }
                            varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                                varn_types::task::TaskState::Resolved(v) => Ok(v),
                                varn_types::task::TaskState::Rejected(v) => Err(v),
                                _ => match ExecCtx::wait_task_handle(handle.clone()) {
                                    Ok(v) => Ok(v),
                                    Err(e) => Err(varn_types::Value::Str(std::rc::Rc::from(e.as_str()))),
                                }
                            },
                            other => Ok(other),
                        };

                        match res_val {
                            Ok(resolved) => {
                                let resolved_nv = fork.heap.intern(resolved);
                                if let Some(frame) = fork.frames.last() {
                                    let base = frame.base;
                                    let slot = base + dest_reg as usize;
                                    if slot < fork.stack.len() {
                                        fork.stack[slot] = resolved_nv;
                                    } else {
                                        fork.stack.resize(slot + 1, VmValue::null());
                                        fork.stack[slot] = resolved_nv;
                                    }
                                }
                            }
                            Err(thrown) => {
                                let thrown_nv = fork.heap.intern(thrown.clone());
                                let err = crate::exec::exceptions::build_thrown_error(
                                    thrown_nv,
                                    &fork.heap,
                                    &fork.frames,
                                );
                                if let Some(handler) = fork.try_handlers.pop() {
                                    while fork.frames.len() > handler.frame_depth {
                                        fork.record_frame_pop();
                                        let f = fork.frames.pop().unwrap();
                                        fork.close_upvalues_above(f.base);
                                    }

                                    let f2 = fork.frames.len() - 1;
                                    let b2 = fork.frames[f2].base;
                                    let required_depth =
                                        b2 + fork.frames[f2].closure.proto.register_count as usize;
                                    fork.stack.truncate(required_depth);
                                    let thrown_val = err.thrown.unwrap_or(VmValue::null());

                                    let slot = b2 + handler.err_reg as usize;
                                    if slot < fork.stack.len() {
                                        fork.stack[slot] = thrown_val;
                                    } else {
                                        fork.stack.resize(slot + 1, VmValue::null());
                                        fork.stack[slot] = thrown_val;
                                    }
                                    let new_frame_idx = fork.frames.len() - 1;
                                    fork.frames[new_frame_idx].ip = handler.catch_ip;
                                } else {
                                    output.reject(thrown);
                                    break;
                                }
                            }
                        }
                    }
                    None => {
                        output.resolve(fork.heap.extract(result));
                        break;
                    }
                    Some(_) => {
                        output.resolve(fork.heap.extract(result));
                        break;
                    }
                },
                Err(err) => {
                    output.reject_msg(err.message);
                    break;
                }
            }
        }

        self.heap = fork.heap;
        self.globals = fork.globals;
        self.modules = fork.modules;

        output
    }

    pub fn exec_run_deferred(&mut self, handle: &varn_types::AsyncTask) {
        let key = handle.ptr_key();
        if let Some(lazy) = self.deferred_tasks.remove(&key) {
            let resolved = self.run_lazy_task_sync(lazy.as_ref());
            match resolved.peek_state() {
                varn_types::task::TaskState::Resolved(v) => handle.resolve(v),
                varn_types::task::TaskState::Rejected(v) => handle.reject(v),
                varn_types::task::TaskState::Pending => {}
            }
        }
    }
}
