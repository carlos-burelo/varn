use std::sync::atomic::Ordering;

use varn_types::task::TaskState;
use varn_types::{AsyncTask, Value};

use super::mount::mount;
use super::pumps::{queue_for, PumpGuard};
use super::queue::{Delivery, ReadyTask, Start, TaskQueue};
use super::suspend::{freeze, thaw};
use crate::exec::ctx::ExecCtx;
use crate::exec::ctx_tasks::Rejection;
use crate::exec::VmSuspend;

const SPARE_VEC_CAPACITY: usize = 1024;

enum ParkAction {
    ResumeOk(Value),
    ResumeErr(Value),
    Park(AsyncTask),
}

fn settle_for_park(ctx: &ExecCtx, value: Value) -> ParkAction {
    match value {
        Value::Task(lazy) => {
            let output = AsyncTask::pending();
            queue_for(ctx).push_ready(ReadyTask {
                start: Start::Fresh(lazy),
                output: output.clone(),
            });
            ParkAction::Park(output)
        }
        Value::TaskHandle(handle) => match handle.peek_state() {
            TaskState::Resolved(v) => ParkAction::ResumeOk(v),
            TaskState::Rejected(v) => ParkAction::ResumeErr(v),
            TaskState::Pending => ParkAction::Park(handle),
        },
        other => ParkAction::ResumeOk(other),
    }
}

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
    ctx.jit_jmp_buf = std::ptr::null_mut();
    ctx.jit_suspend_buf = std::ptr::null_mut();
    ctx.jit_panic_exception_handler = None;
    ctx.jit_panic_exception_error = None;
    ctx.jit_panic_exception_err_obj = None;
    ctx.jit_panic_suspend_resume_ip = None;
    ctx.jit_native_result = crate::value::VmValue::null();
    ctx.osr_request = None;
    ctx.gc_inhibited = false;
}

fn finish(queue: &TaskQueue, mut ctx: Box<ExecCtx>, output: &AsyncTask, res: Result<Value, Value>) {
    recycle(&mut ctx);
    queue.give_spare(ctx);
    output.settle(res);
}

fn failure_message(err: crate::error::RuntimeError) -> Value {
    let mut msg = err.message;
    for frame in &err.frames {
        msg.push_str(&format!(
            "\n  at {} ({}:{})",
            frame.fn_name, frame.file, frame.line
        ));
    }
    Value::Str(std::sync::Arc::from(msg))
}

pub(super) fn run_task(template: &ExecCtx, queue: &TaskQueue, ready: ReadyTask) {
    let ReadyTask { start, output } = ready;
    if !matches!(output.peek_state(), TaskState::Pending) {
        return;
    }
    let mut ctx = queue
        .take_spare()
        .unwrap_or_else(|| Box::new(template.fork_for_task()));
    let mut dest_reg = 0;
    let delivery = match start {
        Start::Fresh(lazy) => {
            mount(&mut ctx, &lazy);
            Delivery::Nothing
        }
        Start::Resume(frozen, delivery) => {
            dest_reg = frozen.dest_reg;
            thaw(&mut ctx, *frozen);
            delivery
        }
    };
    match delivery {
        Delivery::Nothing => {}
        Delivery::Resolved(v) => ctx.resume_with_awaited(dest_reg, v),
        Delivery::Rejected(e) => {
            if let Rejection::Unhandled(thrown) = ctx.reject_awaited(e) {
                return finish(queue, ctx, &output, Err(thrown));
            }
        }
    }
    loop {
        let result = match ctx.run() {
            Ok(result) => result,
            Err(err) => {
                let thrown = match err.thrown {
                    Some(nv) => ctx.heap.extract(nv),
                    None => failure_message(err),
                };
                return finish(queue, ctx, &output, Err(thrown));
            }
        };
        let (value, dest_reg) = match ctx.vm_suspend.take() {
            Some(VmSuspend::Await { value, dest_reg }) => (value, dest_reg),
            None | Some(VmSuspend::Yield { .. }) => {
                let val = ctx.heap.extract(result);
                return finish(queue, ctx, &output, Ok(val));
            }
        };
        match settle_for_park(&ctx, value) {
            ParkAction::ResumeOk(resolved) => ctx.resume_with_awaited(dest_reg, resolved),
            ParkAction::ResumeErr(thrown) => {
                if let Rejection::Unhandled(thrown) = ctx.reject_awaited(thrown) {
                    return finish(queue, ctx, &output, Err(thrown));
                }
            }
            ParkAction::Park(handle) => {
                let stats = &crate::profile::TASK_STATS;
                stats.parks.fetch_add(1, Ordering::Relaxed);
                let yielded = handle.is_yield_token();
                if yielded {
                    stats.yields.fetch_add(1, Ordering::Relaxed);
                }
                let frozen = match freeze(&mut ctx, dest_reg) {
                    Ok(frozen) => frozen,
                    Err(reason) => {
                        let msg = format!("internal: task cannot be suspended: {reason}");
                        return finish(
                            queue,
                            ctx,
                            &output,
                            Err(Value::Str(std::sync::Arc::from(msg))),
                        );
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
                return;
            }
        }
    }
}

impl ExecCtx {
    pub(crate) fn queue(&self) -> &TaskQueue {
        self.task_queue.get_or_init(TaskQueue::new)
    }

    pub fn pump_until(&mut self, target: &AsyncTask) {
        let _guard = PumpGuard::enter(self);
        let queue = self.queue();
        queue.kick_on_settle(target);
        loop {
            if !matches!(target.peek_state(), TaskState::Pending) {
                return;
            }
            let entered = queue.wake_generation();
            queue.wake_settled();
            match queue.pop_ready() {
                Some(ready) => run_task(self, queue, ready),
                None => queue.wait_for_change(entered, || {
                    !matches!(target.peek_state(), TaskState::Pending)
                }),
            }
        }
    }
}
