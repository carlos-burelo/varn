use std::rc::Rc;

use crate::closure::VmClosure;
use crate::error::VmResult;
use crate::heap::HeapObj;
use crate::task::{LazyTask, Status, TaskCell};
use crate::value::VmValue;

use super::ctx::ExecCtx;

pub(crate) enum AwaitStep {
    Ready(VmValue),
    Thrown(VmValue),
    Entered,
    Pending(Rc<TaskCell>),
}

impl ExecCtx {
    pub(crate) fn begin_await(&mut self, value: VmValue, dest_reg: u16) -> VmResult<AwaitStep> {
        if !value.is_heap() {
            return Ok(AwaitStep::Ready(value));
        }
        let step = match self.heap.get(value.as_heap()) {
            Some(HeapObj::Task(lazy)) => {
                let lazy = Rc::clone(lazy);
                if self.call_depth() >= crate::frame::MAX_CALL_DEPTH {
                    return Err(crate::frame::call_depth_exceeded());
                }
                super::scheduler::mount(self, &lazy, dest_reg);
                AwaitStep::Entered
            }
            Some(HeapObj::TaskHandle(cell)) => match cell.status() {
                Status::Resolved => AwaitStep::Ready(cell.value()),
                Status::Rejected => AwaitStep::Thrown(cell.value()),
                Status::Pending => AwaitStep::Pending(Rc::clone(cell)),
            },
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Record(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
                | HeapObj::Range(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => AwaitStep::Ready(value),
        };
        Ok(step)
    }

    pub fn settle_awaited(&mut self, cell: &Rc<TaskCell>) -> Result<VmValue, VmValue> {
        if cell.is_pending() {
            if cell.is_yield() {
                return Ok(VmValue::null());
            }
            self.pump_until(cell);
        }
        match cell.status() {
            Status::Resolved => Ok(cell.value()),
            Status::Rejected => Err(cell.value()),
            Status::Pending => Ok(VmValue::null()),
        }
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

    pub(crate) fn resolve_awaiter(&mut self, dest_reg: u16, value: VmValue) {
        self.resume_with_awaited(dest_reg, value);
        super::scheduler::request_osr(self, varn_types::ssa::OsrMode::Resume);
    }

    pub(crate) fn reject_awaiter(&mut self, thrown: VmValue) {
        self.jit_native_result = thrown;
        self.jit_error_flag = 1;
        super::scheduler::request_osr(self, varn_types::ssa::OsrMode::Resume);
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
            step: None,
        });
        self.pump_until(&output);
        output
    }
}
