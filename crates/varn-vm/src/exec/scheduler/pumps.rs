use std::cell::RefCell;

use super::queue::TaskQueue;
use super::suspend::Frozen;
use crate::exec::ctx::ExecCtx;

struct Pump {
    ctx: *mut ExecCtx,
    queue: *const TaskQueue,
}

thread_local! {
    static PUMPS: RefCell<Vec<Pump>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct PumpGuard;

impl PumpGuard {
    pub(super) fn enter(ctx: &ExecCtx) -> Self {
        let pump = Pump {
            ctx: ctx as *const ExecCtx as *mut ExecCtx,
            queue: ctx.queue() as *const TaskQueue,
        };
        PUMPS.with(|pumps| pumps.borrow_mut().push(pump));
        Self
    }
}

impl Drop for PumpGuard {
    fn drop(&mut self) {
        PUMPS.with(|pumps| {
            pumps.borrow_mut().pop();
        });
    }
}

pub(crate) fn queue_for(exec: &ExecCtx) -> &TaskQueue {
    match PUMPS.with(|pumps| pumps.borrow().last().map(|pump| pump.queue)) {
        Some(queue) => unsafe { &*queue },
        None => exec.queue(),
    }
}

pub(crate) struct GcScope {
    pub(crate) owners: Vec<*mut ExecCtx>,
    pub(crate) frozen: Vec<*mut Frozen>,
}

pub(crate) fn gc_scope(me: &ExecCtx) -> GcScope {
    let mut owners = vec![me as *const ExecCtx as *mut ExecCtx];
    let mut queues: Vec<*const TaskQueue> = Vec::new();
    PUMPS.with(|pumps| {
        for pump in pumps.borrow().iter() {
            if !owners.contains(&pump.ctx) {
                owners.push(pump.ctx);
            }
            if !queues.contains(&pump.queue) {
                queues.push(pump.queue);
            }
        }
    });
    if let Some(own) = me.task_queue.get() {
        let own = own as *const TaskQueue;
        if !queues.contains(&own) {
            queues.push(own);
        }
    }
    let mut frozen = Vec::new();
    for queue in queues {
        unsafe { &*queue }.for_each_frozen(|f| frozen.push(f));
    }
    GcScope { owners, frozen }
}
