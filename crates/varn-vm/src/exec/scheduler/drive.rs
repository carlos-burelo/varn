use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::mount::mount;
use super::pumps::{drain_hosts, host_generation, host_wait, queue_for, PumpGuard};
use super::queue::{Delivery, ReadyTask, Start, TaskQueue};
use super::suspend::{attach_upvalues, detach_upvalues, freeze, thaw};
use crate::exec::ctx::ExecCtx;
use crate::exec::VmSuspend;
use crate::heap::HeapObj;
use crate::task::{settle, LazyTask, Outcome, Status, TaskCell};
use crate::value::VmValue;

const SPARE_VEC_CAPACITY: usize = 1024;
const EAGER_START_DEPTH: u32 = 8;

thread_local! {
    static EAGER_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

struct EagerStart;

impl EagerStart {
    fn enter() -> Option<Self> {
        EAGER_DEPTH.with(|depth| {
            let now = depth.get();
            (now < EAGER_START_DEPTH).then(|| {
                depth.set(now + 1);
                EagerStart
            })
        })
    }
}

impl Drop for EagerStart {
    fn drop(&mut self) {
        EAGER_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

enum ParkAction {
    ResumeOk(VmValue),
    ResumeErr(VmValue),
    Park(Rc<TaskCell>),
}

fn settle_for_park(ctx: &mut ExecCtx, value: VmValue) -> ParkAction {
    if value.is_heap() {
        match ctx.heap.get(value.as_heap()) {
            Some(HeapObj::Task(lazy)) => {
                let lazy = Rc::clone(lazy);
                return match start_now(ctx, lazy) {
                    Ok(output) => match output.status() {
                        Status::Resolved => ParkAction::ResumeOk(output.value()),
                        Status::Rejected => ParkAction::ResumeErr(output.value()),
                        Status::Pending => ParkAction::Park(output),
                    },
                    Err(lazy) => {
                        let output = TaskCell::pending();
                        queue_for(ctx).push_ready(ReadyTask {
                            start: Start::Fresh(lazy),
                            output: Rc::clone(&output),
                        });
                        ParkAction::Park(output)
                    }
                };
            }
            Some(HeapObj::TaskHandle(cell)) => {
                let cell = Rc::clone(cell);
                return match cell.status() {
                    Status::Resolved => ParkAction::ResumeOk(cell.value()),
                    Status::Rejected => ParkAction::ResumeErr(cell.value()),
                    Status::Pending => ParkAction::Park(cell),
                };
            }
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
                | HeapObj::Symbol(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => {}
        }
    }
    ParkAction::ResumeOk(value)
}

pub(crate) fn start_now(
    ctx: &mut ExecCtx,
    lazy: Rc<LazyTask>,
) -> Result<Rc<TaskCell>, Rc<LazyTask>> {
    let Some(_depth) = EagerStart::enter() else {
        return Err(lazy);
    };
    let queue: *const TaskQueue = queue_for(ctx);
    let queue = unsafe { &*queue };
    let output = TaskCell::pending();
    let open = std::mem::take(&mut ctx.open_upvalues);
    detach_upvalues(ctx, &open);
    {
        let _held = PumpGuard::hold(ctx, queue);
        run_task(
            ctx,
            queue,
            ReadyTask {
                start: Start::Fresh(lazy),
                output: Rc::clone(&output),
            },
        );
    }
    attach_upvalues(ctx, &open);
    ctx.open_upvalues = open;
    Ok(output)
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
            mount(&mut ctx, &lazy);
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
            ctx.resume_with_awaited(dest_reg, cell.value());
            request_osr(&mut ctx, varn_types::ssa::OsrMode::Resume);
        } else {
            ctx.jit_native_result = cell.value();
            ctx.jit_error_flag = 1;
            request_osr(&mut ctx, varn_types::ssa::OsrMode::Resume);
        }
    } else if resumed {
        request_osr(&mut ctx, varn_types::ssa::OsrMode::Resume);
    }
    loop {
        let result = match ctx.run() {
            Ok(result) => result,
            Err(err) => {
                let thrown = failure_value(&mut ctx, err);
                return finish(queue, ctx, &output, Err(thrown));
            }
        };
        let (value, dest_reg) = match ctx.vm_suspend.take() {
            Some(VmSuspend::Await { value, dest_reg }) => (value, dest_reg),
            None | Some(VmSuspend::Yield { .. }) | Some(VmSuspend::DebugBreak) => {
                return finish(queue, ctx, &output, Ok(result));
            }
        };
        match settle_for_park(&mut ctx, value) {
            ParkAction::ResumeOk(resolved) => {
                ctx.resume_with_awaited(dest_reg, resolved);
                request_osr(&mut ctx, varn_types::ssa::OsrMode::Resume);
            }
            ParkAction::ResumeErr(thrown) => {
                ctx.jit_native_result = thrown;
                ctx.jit_error_flag = 1;
                request_osr(&mut ctx, varn_types::ssa::OsrMode::Resume);
            }
            ParkAction::Park(handle) => {
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
                return;
            }
        }
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
