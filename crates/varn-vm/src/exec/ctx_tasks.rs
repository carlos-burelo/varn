use std::rc::Rc;

use crate::closure::VmClosure;
use crate::heap::HeapObj;
use crate::task::{LazyTask, Status, TaskCell};
use crate::value::VmValue;

use super::ctx::ExecCtx;

pub(crate) enum Rejection {
    Caught,
    Unhandled(VmValue),
}

impl ExecCtx {
    pub fn settle_awaited(&mut self, value: VmValue) -> Result<VmValue, VmValue> {
        if value.is_heap() {
            let cell = match self.heap.get_by_idx(value.as_heap_idx()) {
                Some(HeapObj::Task(lazy)) => Some(self.run_lazy_task_sync(Rc::clone(lazy))),
                Some(HeapObj::TaskHandle(cell)) => Some(Rc::clone(cell)),
                _ => None,
            };
            if let Some(cell) = cell {
                if cell.is_pending() {
                    if cell.is_yield() {
                        return Ok(VmValue::null());
                    }
                    self.pump_until(&cell);
                }
                return match cell.status() {
                    Status::Resolved => Ok(cell.value()),
                    Status::Rejected => Err(cell.value()),
                    Status::Pending => Ok(VmValue::null()),
                };
            }
        }
        Ok(value)
    }

    pub(crate) fn resume_with_awaited(&mut self, dest_reg: u16, resolved: VmValue) {
        let Some(frame) = self.frames.last() else {
            return;
        };
        let (base, nregs) = (frame.base, frame.closure().proto.register_count as usize);
        self.stack.ensure_frame_size(base, nregs);
        if (dest_reg as usize) < nregs {
            let _ = self.stack.unbox_into_reg(base, dest_reg as usize, resolved);
        }
    }

    pub(crate) fn reject_awaited(&mut self, thrown: VmValue) -> Rejection {
        let err = crate::exec::exceptions::build_thrown_error(thrown, &self.heap, &self.frames);
        match self.try_handlers.pop() {
            Some(handler) => {
                let thrown_val = err.thrown.unwrap_or(VmValue::null());
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

    pub fn run_lazy_task_sync(&mut self, task: Rc<LazyTask>) -> Rc<TaskCell> {
        let output = TaskCell::pending();
        self.queue().push_ready(super::scheduler::ReadyTask {
            start: super::scheduler::Start::Fresh(task),
            output: Rc::clone(&output),
        });
        self.pump_until(&output);
        output
    }
}
