use std::rc::Rc;

use crate::frame::CallFrame;

use crate::closure::{VmClosure, VmUpvalue};
use crate::value::VmValue;

use super::ctx::ExecCtx;
use super::VmSuspend;

/// What happened to a rejection delivered into a suspended frame.
pub(crate) enum Rejection {
    /// A `try` handler in the suspended frame took it; the context can run on.
    Caught,
    /// Nothing caught it — the caller decides what "unhandled" means.
    Unhandled(varn_types::Value),
}

impl ExecCtx {
    /// Drive an awaited value to a settled state, blocking the thread if the
    /// task is still pending.
    ///
    /// Shared by [`Self::run_lazy_task_sync`] and the async generator driver:
    /// `await` means the same thing in an async function body and in an
    /// `async function*` body, and writing it twice is how the two would
    /// drift.
    pub(crate) fn settle_awaited(
        &mut self,
        value: varn_types::Value,
    ) -> Result<varn_types::Value, varn_types::Value> {
        match value {
            varn_types::Value::Task(lazy) => {
                let handle = self.run_lazy_task_sync(lazy.as_ref());
                match handle.peek_state() {
                    varn_types::task::TaskState::Resolved(v) => Ok(v),
                    varn_types::task::TaskState::Rejected(v) => Err(v),
                    varn_types::task::TaskState::Pending => Ok(varn_types::Value::Null),
                }
            }
            varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                varn_types::task::TaskState::Resolved(v) => Ok(v),
                varn_types::task::TaskState::Rejected(v) => Err(v),
                varn_types::task::TaskState::Pending => Self::wait_task_handle_value(handle),
            },
            // `await` on anything else is the identity — that is what makes
            // awaiting a plain value, or a `next()` that already settled, a
            // no-op rather than an error.
            other => Ok(other),
        }
    }

    /// Put a settled `await` result where the suspended frame expects it.
    pub(crate) fn resume_with_awaited(&mut self, dest_reg: u16, resolved: varn_types::Value) {
        let resolved = crate::exec::host_values::open_resolved(self, resolved);
        let nv = self.heap.intern(resolved);
        let Some(frame) = self.frames.last() else {
            return;
        };
        let slot = frame.base + dest_reg as usize;
        if slot >= self.stack.len() {
            self.stack.resize(slot + 1, VmValue::null());
        }
        self.stack[slot] = nv;
    }

    /// Deliver a rejected `await` into the suspended frame, unwinding to its
    /// nearest `try` handler if it has one.
    pub(crate) fn reject_awaited(&mut self, thrown: varn_types::Value) -> Rejection {
        let thrown = crate::exec::host_values::open_rejected(self, thrown);
        let thrown_nv = self.heap.intern(thrown.clone());
        let err = crate::exec::exceptions::build_thrown_error(thrown_nv, &self.heap, &self.frames);
        match self.try_handlers.pop() {
            Some(handler) => {
                let thrown_val = err.thrown.unwrap_or(VmValue::null());
                crate::exec::frame_ctrl::unwind_to_handler(self, handler, thrown_val);
                Rejection::Caught
            }
            None => Rejection::Unhandled(thrown),
        }
    }

    pub fn wait_task_handle(task: varn_types::AsyncTask) -> Result<varn_types::Value, String> {
        Self::wait_task_handle_value(task).map_err(|v| format!("{v}"))
    }

    /// Like [`Self::wait_task_handle`] but preserves the rejection `Value`
    /// instead of stringifying it — required so typed payloads (e.g.
    /// `HostError`) survive to the await-resume hook (`host_values`).
    pub fn wait_task_handle_value(
        task: varn_types::AsyncTask,
    ) -> Result<varn_types::Value, varn_types::Value> {
        match task.peek_state() {
            varn_types::task::TaskState::Resolved(v) => return Ok(v),
            varn_types::task::TaskState::Rejected(v) => return Err(v),
            varn_types::task::TaskState::Pending => {}
        }

        let (tx, rx) = std::sync::mpsc::channel();
        task.on_settle(move |result| {
            let _ = tx.send(result);
        });

        match rx.recv() {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(v)) => Err(v),
            Err(_) => Err(varn_types::Value::Str(std::rc::Rc::from("task dropped"))),
        }
    }

    pub(crate) fn trace_event(
        &self,
        label: &str,
        frame_idx: usize,
        closure: &VmClosure,
        op_ip: usize,
        op: Option<varn_core::OpCode>,
    ) {
        if !self.settings.trace {
            return;
        }

        let fn_name = closure.proto.name.as_deref().unwrap_or("<anon>");
        let line = closure.proto.chunk.lines.get_line(op_ip);
        varn_term::terminal::tagged(
            format_args!("vm:{label}"),
            format_args!(
                "fn={fn_name} file={} frame={} ip={} line={} stack={} tries={} op={:?}",
                closure.proto.chunk.source_file,
                frame_idx,
                op_ip,
                line,
                self.stack.len(),
                self.try_handlers.len(),
                op,
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
        let mut proto = Rc::clone(&task.closure.proto);
        crate::globals::resolve_in_proto(Rc::make_mut(&mut proto), &mut fork.globals);
        let closure = Rc::new(VmClosure::with_upvalues(
            proto,
            upvalues,
            Rc::new(constants),
            fork.settings,
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
        let mut frame = CallFrame::new_owned(closure, 0);
        frame.current_class = task.current_class.clone();
        fork.frames.push(frame);

        let output = varn_types::AsyncTask::pending();
        loop {
            match fork.run() {
                Ok(result) => match fork.vm_suspend.take() {
                    Some(VmSuspend::Await { value, dest_reg }) => {
                        match fork.settle_awaited(value) {
                            Ok(resolved) => fork.resume_with_awaited(dest_reg, resolved),
                            Err(thrown) => match fork.reject_awaited(thrown) {
                                Rejection::Caught => {}
                                Rejection::Unhandled(thrown) => {
                                    output.reject(thrown);
                                    break;
                                }
                            },
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
                    let mut msg = err.message;
                    for frame in &err.frames {
                        msg.push_str(&format!(
                            "\n  at {} ({}:{})",
                            frame.fn_name, frame.file, frame.line
                        ));
                    }
                    output.reject_msg(msg);
                    break;
                }
            }
        }

        self.heap = fork.heap;
        self.globals = fork.globals;
        self.modules = fork.modules;

        output
    }
}
