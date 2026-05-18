use crate::runner::TaskRunner;
use crate::suspend::Suspend;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use varn_types::task::{AsyncTask, TaskState};
use varn_types::Value;

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub poll_budget: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self { poll_budget: 256 }
    }
}

impl SchedulerConfig {
    pub fn normalized_poll_budget(&self) -> usize {
        if self.poll_budget == 0 {
            1
        } else {
            self.poll_budget
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SchedulerMetricsSnapshot {
    pub root_tasks: u64,
    pub spawned_tasks: u64,
    pub spawned_async_gens: u64,
    pub vm_polls: u64,
    pub task_waits: u64,
    pub timer_waits: u64,
    pub task_yields: u64,
    pub task_resolved: u64,
    pub task_rejected: u64,
    pub task_cancelled: u64,
    pub async_gen_yields: u64,
    pub async_gen_resolved: u64,
    pub async_gen_rejected: u64,
    pub cooperative_yields: u64,
}

#[derive(Default)]
struct SchedulerMetrics {
    root_tasks: AtomicU64,
    spawned_tasks: AtomicU64,
    spawned_async_gens: AtomicU64,
    vm_polls: AtomicU64,
    task_waits: AtomicU64,
    timer_waits: AtomicU64,
    task_yields: AtomicU64,
    task_resolved: AtomicU64,
    task_rejected: AtomicU64,
    task_cancelled: AtomicU64,
    async_gen_yields: AtomicU64,
    async_gen_resolved: AtomicU64,
    async_gen_rejected: AtomicU64,
    cooperative_yields: AtomicU64,
}

impl SchedulerMetrics {
    fn snapshot(&self) -> SchedulerMetricsSnapshot {
        SchedulerMetricsSnapshot {
            root_tasks: self.root_tasks.load(Ordering::Relaxed),
            spawned_tasks: self.spawned_tasks.load(Ordering::Relaxed),
            spawned_async_gens: self.spawned_async_gens.load(Ordering::Relaxed),
            vm_polls: self.vm_polls.load(Ordering::Relaxed),
            task_waits: self.task_waits.load(Ordering::Relaxed),
            timer_waits: self.timer_waits.load(Ordering::Relaxed),
            task_yields: self.task_yields.load(Ordering::Relaxed),
            task_resolved: self.task_resolved.load(Ordering::Relaxed),
            task_rejected: self.task_rejected.load(Ordering::Relaxed),
            task_cancelled: self.task_cancelled.load(Ordering::Relaxed),
            async_gen_yields: self.async_gen_yields.load(Ordering::Relaxed),
            async_gen_resolved: self.async_gen_resolved.load(Ordering::Relaxed),
            async_gen_rejected: self.async_gen_rejected.load(Ordering::Relaxed),
            cooperative_yields: self.cooperative_yields.load(Ordering::Relaxed),
        }
    }
}

pub struct Scheduler {
    root: Option<(Box<dyn TaskRunner>, AsyncTask)>,
    config: SchedulerConfig,
    metrics: Rc<SchedulerMetrics>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            root: None,
            config: SchedulerConfig::default(),
            metrics: Rc::new(SchedulerMetrics::default()),
        }
    }

    pub fn with_config(config: SchedulerConfig) -> Self {
        Self {
            root: None,
            config,
            metrics: Rc::new(SchedulerMetrics::default()),
        }
    }

    pub fn set_config(&mut self, config: SchedulerConfig) {
        self.config = config;
    }

    pub fn metrics_snapshot(&self) -> SchedulerMetricsSnapshot {
        self.metrics.snapshot()
    }

    pub fn spawn_root(&mut self, vm: Box<dyn TaskRunner>, output: AsyncTask) {
        self.root = Some((vm, output));
    }

    pub fn run(self) -> Result<Value, Value> {
        self.run_with_metrics().0
    }

    pub fn run_with_metrics(self) -> (Result<Value, Value>, SchedulerMetricsSnapshot) {
        let (vm, output) = match self.root {
            Some(r) => r,
            None => return (Ok(Value::Null), self.metrics.snapshot()),
        };

        let poll_budget = self.config.normalized_poll_budget();
        let metrics = Rc::clone(&self.metrics);
        metrics.root_tasks.fetch_add(1, Ordering::Relaxed);

        static SHARED_RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        let rt = SHARED_RT.get_or_init(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
        });

        let local = tokio::task::LocalSet::new();
        let result = local.block_on(rt, async move {
            tokio::task::spawn_local(run_task(
                vm,
                output.clone(),
                poll_budget,
                Rc::clone(&metrics),
            ));
            await_varn_task(output).await
        });

        (result, self.metrics.snapshot())
    }
}

enum WaitOutcome {
    Ready(Result<Value, Value>),
    Cancelled,
}

async fn run_task(
    mut vm: Box<dyn TaskRunner>,
    output: AsyncTask,
    poll_budget: usize,
    metrics: Rc<SchedulerMetrics>,
) {
    let mut polls_since_yield = 0usize;

    loop {
        if !output.is_pending() {
            metrics.task_cancelled.fetch_add(1, Ordering::Relaxed);
            return;
        }

        metrics.vm_polls.fetch_add(1, Ordering::Relaxed);
        let (poll, suspend) = vm.poll_vm();
        drain_spawns(&mut vm, poll_budget, &metrics);

        match poll {
            varn_types::Poll::Ready(Ok(v)) => {
                metrics.task_resolved.fetch_add(1, Ordering::Relaxed);
                output.resolve(v);
                return;
            }
            varn_types::Poll::Ready(Err(e)) => {
                metrics.task_rejected.fetch_add(1, Ordering::Relaxed);
                output.reject_msg(e);
                return;
            }
            varn_types::Poll::Pending => {}
        }

        polls_since_yield += 1;
        if polls_since_yield >= poll_budget {
            polls_since_yield = 0;
            metrics.cooperative_yields.fetch_add(1, Ordering::Relaxed);
            tokio::task::yield_now().await;
            if !output.is_pending() {
                metrics.task_cancelled.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }

        match suspend {
            Some(Suspend::Task(fut)) => {
                metrics.task_waits.fetch_add(1, Ordering::Relaxed);
                match await_varn_task_result_or_cancel(fut, output.clone()).await {
                    WaitOutcome::Ready(r) => {
                        if let Err(e) = vm.push_resume_value(r) {
                            metrics.task_rejected.fetch_add(1, Ordering::Relaxed);
                            output.reject_msg(e);
                            return;
                        }
                    }
                    WaitOutcome::Cancelled => {
                        metrics.task_cancelled.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                }
            }
            Some(Suspend::Timer(dur)) => {
                metrics.timer_waits.fetch_add(1, Ordering::Relaxed);
                if !sleep_or_cancel(dur, output.clone()).await {
                    metrics.task_cancelled.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                if let Err(e) = vm.push_resume_value(Ok(Value::Null)) {
                    metrics.task_rejected.fetch_add(1, Ordering::Relaxed);
                    output.reject_msg(e);
                    return;
                }
            }
            Some(Suspend::Yield(val)) => {
                metrics.task_yields.fetch_add(1, Ordering::Relaxed);
                metrics.task_resolved.fetch_add(1, Ordering::Relaxed);
                output.resolve(val);
                return;
            }
            None => {
                metrics.task_resolved.fetch_add(1, Ordering::Relaxed);
                output.resolve(Value::Null);
                return;
            }
        }
    }
}

pub async fn run_task_standalone(vm: Box<dyn TaskRunner>, output: AsyncTask) {
    let poll_budget = SchedulerConfig::default().normalized_poll_budget();
    run_task(
        vm,
        output,
        poll_budget,
        Rc::new(SchedulerMetrics::default()),
    )
    .await;
}

async fn run_async_gen_task(
    mut vm: Box<dyn TaskRunner>,
    poll_budget: usize,
    metrics: Rc<SchedulerMetrics>,
) {
    let gen_channel = match vm.gen_channel() {
        Some(ch) => ch,
        None => return,
    };

    let mut polls_since_yield = 0usize;

    loop {
        if gen_channel.is_done() {
            return;
        }

        metrics.vm_polls.fetch_add(1, Ordering::Relaxed);
        let (poll, suspend) = vm.poll_vm();
        drain_spawns(&mut vm, poll_budget, &metrics);

        match poll {
            varn_types::Poll::Ready(Ok(v)) => {
                metrics.async_gen_resolved.fetch_add(1, Ordering::Relaxed);
                gen_channel.mark_done();
                if let Some(out) = gen_channel.output.borrow_mut().take() {
                    out.resolve(make_iter_result(v, true));
                }
                return;
            }
            varn_types::Poll::Ready(Err(e)) => {
                metrics.async_gen_rejected.fetch_add(1, Ordering::Relaxed);
                gen_channel.mark_done();
                if let Some(out) = gen_channel.output.borrow_mut().take() {
                    out.reject_msg(e);
                }
                return;
            }
            varn_types::Poll::Pending => {}
        }

        polls_since_yield += 1;
        if polls_since_yield >= poll_budget {
            polls_since_yield = 0;
            metrics.cooperative_yields.fetch_add(1, Ordering::Relaxed);
            tokio::task::yield_now().await;
            if gen_channel.is_done() {
                return;
            }
        }

        match suspend {
            Some(Suspend::Yield(val)) => {
                metrics.async_gen_yields.fetch_add(1, Ordering::Relaxed);
                if let Some(out) = gen_channel.output.borrow_mut().take() {
                    out.resolve(make_iter_result(val, false));
                }
                let wake = AsyncTask::pending();
                *gen_channel.wake_signal.borrow_mut() = Some(wake.clone());
                if gen_channel.is_done() {
                    return;
                }
                match await_varn_task_result_or_cancel(wake, gen_channel.cancel_signal.clone())
                    .await
                {
                    WaitOutcome::Ready(_) => {}
                    WaitOutcome::Cancelled => return,
                }
            }
            Some(Suspend::Task(fut)) => {
                metrics.task_waits.fetch_add(1, Ordering::Relaxed);
                match await_varn_task_result_or_cancel(fut, gen_channel.cancel_signal.clone()).await
                {
                    WaitOutcome::Ready(r) => {
                        if let Err(e) = vm.push_resume_value(r) {
                            metrics.async_gen_rejected.fetch_add(1, Ordering::Relaxed);
                            gen_channel.mark_done();
                            if let Some(out) = gen_channel.output.borrow_mut().take() {
                                out.reject_msg(e);
                            }
                            return;
                        }
                    }
                    WaitOutcome::Cancelled => return,
                }
            }
            Some(Suspend::Timer(dur)) => {
                metrics.timer_waits.fetch_add(1, Ordering::Relaxed);
                if !sleep_or_cancel(dur, gen_channel.cancel_signal.clone()).await {
                    return;
                }
                if gen_channel.is_done() {
                    return;
                }
                if let Err(e) = vm.push_resume_value(Ok(Value::Null)) {
                    metrics.async_gen_rejected.fetch_add(1, Ordering::Relaxed);
                    gen_channel.mark_done();
                    if let Some(out) = gen_channel.output.borrow_mut().take() {
                        out.reject_msg(e);
                    }
                    return;
                }
            }
            None => {
                metrics.async_gen_resolved.fetch_add(1, Ordering::Relaxed);
                gen_channel.mark_done();
                if let Some(out) = gen_channel.output.borrow_mut().take() {
                    out.resolve(make_iter_result(Value::Null, true));
                }
                return;
            }
        }
    }
}

fn drain_spawns(vm: &mut Box<dyn TaskRunner>, poll_budget: usize, metrics: &Rc<SchedulerMetrics>) {
    for (child_vm, child_out) in vm.take_pending_spawns() {
        metrics.spawned_tasks.fetch_add(1, Ordering::Relaxed);
        tokio::task::spawn_local(run_task(
            child_vm,
            child_out,
            poll_budget,
            Rc::clone(metrics),
        ));
    }
    for (_, gen_vm) in vm.take_pending_async_gen_spawns() {
        metrics.spawned_async_gens.fetch_add(1, Ordering::Relaxed);
        tokio::task::spawn_local(run_async_gen_task(gen_vm, poll_budget, Rc::clone(metrics)));
    }
}

async fn await_varn_task(fut: AsyncTask) -> Result<Value, Value> {
    await_varn_task_result(fut).await
}

async fn await_varn_task_result(fut: AsyncTask) -> Result<Value, Value> {
    match fut.peek_state() {
        TaskState::Resolved(v) => return Ok(v),
        TaskState::Rejected(v) => return Err(v),
        TaskState::Pending => {}
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    fut.on_settle(move |r| {
        let _ = tx.send(r);
    });
    match rx.await {
        Ok(r) => r,
        Err(_) => Err(Value::Str(Rc::from("task dropped"))),
    }
}

async fn await_varn_task_settle(fut: AsyncTask) {
    let _ = await_varn_task_result(fut).await;
}

async fn await_varn_task_result_or_cancel(fut: AsyncTask, cancel_signal: AsyncTask) -> WaitOutcome {
    if !cancel_signal.is_pending() {
        return WaitOutcome::Cancelled;
    }

    tokio::select! {
        r = await_varn_task_result(fut) => WaitOutcome::Ready(r),
        _ = await_varn_task_settle(cancel_signal) => WaitOutcome::Cancelled,
    }
}

async fn sleep_or_cancel(dur: Duration, cancel_signal: AsyncTask) -> bool {
    if !cancel_signal.is_pending() {
        return false;
    }

    tokio::select! {
        _ = tokio::time::sleep(dur) => true,
        _ = await_varn_task_settle(cancel_signal) => false,
    }
}

fn make_iter_result(value: Value, done: bool) -> Value {
    let mut obj = varn_types::value::ObjData::new();
    obj.set_field(Rc::from("value"), value);
    obj.set_field(Rc::from("done"), Value::Bool(done));
    varn_types::value::new_object(obj)
}
