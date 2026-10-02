use crate::closure::VmClosure;
use crate::value::VmValue;

use super::ctx::ExecCtx;

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
                let handle = self.run_lazy_task_sync(lazy);
                match handle.peek_state() {
                    varn_types::task::TaskState::Resolved(v) => Ok(v),
                    varn_types::task::TaskState::Rejected(v) => Err(v),
                    varn_types::task::TaskState::Pending => Ok(varn_types::Value::Null),
                }
            }
            varn_types::Value::TaskHandle(handle) => match handle.peek_state() {
                varn_types::task::TaskState::Resolved(v) => Ok(v),
                varn_types::task::TaskState::Rejected(v) => Err(v),
                varn_types::task::TaskState::Pending => {
                    if handle.is_yield_token() {
                        return Ok(varn_types::Value::Null);
                    }
                    self.pump_until(&handle);
                    match handle.peek_state() {
                        varn_types::task::TaskState::Resolved(v) => Ok(v),
                        varn_types::task::TaskState::Rejected(v) => Err(v),
                        varn_types::task::TaskState::Pending => Ok(varn_types::Value::Null),
                    }
                }
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
        let (base, nregs) = (frame.base, frame.closure().proto.register_count as usize);
        self.stack.ensure_frame_size(base, nregs);
        if (dest_reg as usize) < nregs {
            // Como en el resume de generadores: en programas bien tipados no
            // falla; si el valor no encaja se ignora y el programa verá el
            // default del slot (el `await` ya se resolvió).
            let _ = self.stack.unbox_into_reg(base, dest_reg as usize, nv);
        }
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
                // `err_reg` es `Dynamic` por construcción: infalible en la
                // práctica. Si fallara, el rechazo queda sin manejar.
                if crate::exec::frame_ctrl::unwind_to_handler(self, handler, thrown_val).is_err() {
                    return Rejection::Unhandled(thrown);
                }
                Rejection::Caught
            }
            None => Rejection::Unhandled(thrown),
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
        varn_core::term::terminal::tagged(
            format_args!("vm:{label}"),
            format_args!(
                "fn={fn_name} file={} frame={} ip={} line={} stack={} tries={} op={:?}",
                closure.proto.chunk.source_file,
                frame_idx,
                op_ip,
                line,
                self.stack.frame_count(),
                self.try_handlers.len(),
                op,
            ),
        );
    }

    pub fn run_lazy_task_sync(
        &mut self,
        task: std::rc::Rc<varn_types::value::LazyTask>,
    ) -> varn_types::AsyncTask {
        let output = varn_types::AsyncTask::pending();
        self.queue().push_ready(super::scheduler::ReadyTask {
            start: super::scheduler::Start::Fresh(task),
            output: output.clone(),
        });
        self.pump_until(&output);
        output
    }
}
