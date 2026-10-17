use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::mount::mount;
use super::pumps::{drain_hosts, host_generation, host_wait, PumpGuard};
use super::queue::{Delivery, ReadyTask, Start, TaskQueue};
use super::suspend::{freeze, thaw};
use crate::exec::ctx::ExecCtx;
use crate::exec::VmSuspend;
use crate::task::{settle, Outcome, Status, TaskCell};
use crate::value::VmValue;

const SPARE_VEC_CAPACITY: usize = 1024;
fn recycle(ctx: &mut ExecCtx) {
    ctx.stack.gpr.clear();
    ctx.stack.fpr.clear();
    ctx.stack.refs.clear();
    ctx.stack.dyn_.clear();
    ctx.stack.allocs.clear();
    ctx.stack.gpr.shrink_to(SPARE_VEC_CAPACITY);
    ctx.stack.fpr.shrink_to(SPARE_VEC_CAPACITY);
    ctx.stack.refs.shrink_to(SPARE_VEC_CAPACITY);
    ctx.stack.dyn_.shrink_to(SPARE_VEC_CAPACITY);
    ctx.frames.clear();
    ctx.try_handlers.clear();
    ctx.open_upvalues.clear();
    ctx.pending_constructors.clear();
    ctx.pending_setters.clear();
    ctx.module_exports.clear();
    ctx.stage.clear();
    ctx.vm_suspend = None;
    ctx.jit_suspend_buf = std::ptr::null_mut();
    ctx.jit_panic_exception_error = None;
    ctx.jit_panic_exception_err_obj = None;
    ctx.jit_panic_suspend_resume_ip = None;
    ctx.jit_native_result = VmValue::null();
    ctx.jit_error_flag = 0;
    ctx.osr_request = None;
    ctx.gc_inhibited = false;
}

fn finish(queue: &TaskQueue, mut ctx: Box<ExecCtx>, output: &Rc<TaskCell>, res: Outcome) {
    settle(&mut ctx.heap, output, res);
    recycle(&mut ctx);
    queue.give_spare(ctx);
}

fn failure_value(ctx: &mut ExecCtx, err: crate::error::RuntimeError) -> VmValue {
    crate::exec::exceptions::thrown_value_for(&err, &mut ctx.heap)
}

pub(crate) fn request_osr(ctx: &mut ExecCtx, mode: varn_types::ssa::OsrMode) {
    if let Some(frame) = ctx.frames.last()
        && frame.ip != 0
    {
        ctx.osr_request = Some(varn_types::ssa::OsrRequest { ip: frame.ip, mode });
    }
}

pub(super) fn run_task(template: &ExecCtx, queue: &TaskQueue, ready: ReadyTask) {
    let ReadyTask { start, output } = ready;
    if !output.is_pending() {
        return;
    }
    let mut ctx = queue
        .take_spare()
        .unwrap_or_else(|| Box::new(template.fork_for_task()));
    let mut dest_reg = 0;
    let resumed = matches!(start, Start::Resume(_, _));
    let delivery = match start {
        Start::Fresh(lazy) => {
            mount(&mut ctx, &lazy, crate::frame::CallFrame::NO_RETURN_REG);
            Delivery::Nothing
        }
        Start::Resume(frozen, delivery) => {
            dest_reg = frozen.dest_reg;
            thaw(&mut ctx, *frozen);
            delivery
        }
    };
    if let Delivery::Settled(cell) = delivery {
        if matches!(cell.status(), Status::Resolved) {
            ctx.resolve_awaiter(dest_reg, cell.value());
        } else {
            ctx.reject_awaiter(cell.value());
        }
    } else if resumed {
        request_osr(&mut ctx, varn_types::ssa::OsrMode::Resume);
    }
    let result = match ctx.run() {
        Ok(result) => result,
        Err(err) => {
            let thrown = failure_value(&mut ctx, err);
            return finish(queue, ctx, &output, Err(thrown));
        }
    };
    let (handle, dest_reg) = match ctx.vm_suspend.take() {
        Some(VmSuspend::Await { cell, dest_reg }) => (cell, dest_reg),
        None | Some(VmSuspend::Yield { .. }) | Some(VmSuspend::DebugBreak) => {
            return finish(queue, ctx, &output, Ok(result));
        }
    };
    let stats = &crate::profile::TASK_STATS;
    stats.parks.fetch_add(1, Ordering::Relaxed);
    let yielded = handle.is_yield();
    if yielded {
        stats.yields.fetch_add(1, Ordering::Relaxed);
    }
    let frozen = match freeze(&mut ctx, dest_reg) {
        Ok(frozen) => frozen,
        Err(reason) => {
            let msg = format!("internal: task cannot be suspended: {reason}");
            let err = ctx.heap.alloc_str(&msg);
            return finish(queue, ctx, &output, Err(err));
        }
    };
    stats.released.fetch_add(1, Ordering::Relaxed);
    recycle(&mut ctx);
    queue.give_spare(ctx);
    if yielded {
        queue.push_ready(ReadyTask {
            start: Start::Resume(frozen, Delivery::Nothing),
            output,
        });
    } else {
        queue.park(output, handle, frozen);
    }
}

impl ExecCtx {
    pub(crate) fn queue(&self) -> &TaskQueue {
        self.task_queue.get_or_init(TaskQueue::new)
    }

    pub fn pump_until(&mut self, target: &Rc<TaskCell>) {
        let _guard = PumpGuard::enter(self);
        let queue: *const TaskQueue = self.queue();
        let queue = unsafe { &*queue };
        loop {
            if !target.is_pending() {
                return;
            }
            let entered = host_generation();
            for cell in drain_hosts() {
                crate::exec::host_tasks::complete_host(self, &cell);
            }
            queue.collect_resumes();
            match queue.pop_ready() {
                Some(ready) => run_task(self, queue, ready),
                None => host_wait(entered, || !target.is_pending() || queue.has_inbox()),
            }
        }
    }
}
