use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use varn_types::{HostPromise, WakeQueue, WakeToken};

use super::queue::TaskQueue;
use super::suspend::Frozen;
use crate::exec::ctx::ExecCtx;
use crate::task::{LazyTask, TaskCell};

struct Pump {
    ctx: *mut ExecCtx,
    queue: *const TaskQueue,
}

struct Hosts {
    wakes: Arc<WakeQueue>,
    cells: Vec<Option<Rc<TaskCell>>>,
    free: Vec<u32>,
}

thread_local! {
    static PUMPS: RefCell<Vec<Pump>> = const { RefCell::new(Vec::new()) };
    static HOSTS: RefCell<Option<Hosts>> = const { RefCell::new(None) };
}

fn with_hosts<R>(f: impl FnOnce(&mut Hosts) -> R) -> R {
    HOSTS.with(|hosts| {
        let mut guard = hosts.borrow_mut();
        let state = guard.get_or_insert_with(|| Hosts {
            wakes: WakeQueue::new(),
            cells: Vec::new(),
            free: Vec::new(),
        });
        f(state)
    })
}

pub(crate) fn adopt_host(cell: Rc<TaskCell>, promise: &HostPromise) {
    let (wakes, index) = with_hosts(|hosts| {
        let index = match hosts.free.pop() {
            Some(index) => {
                hosts.cells[index as usize] = Some(cell);
                index
            }
            None => {
                hosts.cells.push(Some(cell));
                hosts.cells.len() as u32 - 1
            }
        };
        (Arc::clone(&hosts.wakes), index)
    });
    promise.watch(WakeToken::new(&wakes, u64::from(index)));
}

pub(crate) fn host_generation() -> u64 {
    with_hosts(|hosts| hosts.wakes.generation())
}

pub(crate) fn host_wait(entered: u64, done: impl Fn() -> bool) {
    let wakes = with_hosts(|hosts| Arc::clone(&hosts.wakes));
    wakes.wait_past(entered, done);
}

pub(crate) fn drain_hosts() -> Vec<Rc<TaskCell>> {
    with_hosts(|hosts| {
        let mut tokens = std::collections::VecDeque::new();
        hosts.wakes.drain_into(&mut tokens);
        let mut done = Vec::with_capacity(tokens.len());
        for token in tokens {
            let index = token as u32;
            if let Some(cell) = hosts.cells.get_mut(index as usize).and_then(Option::take) {
                hosts.free.push(index);
                done.push(cell);
            }
        }
        done
    })
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
    pub(crate) cells: Vec<Rc<TaskCell>>,
    pub(crate) lazies: Vec<Rc<LazyTask>>,
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
    let mut scope = GcScope {
        owners,
        frozen: Vec::new(),
        cells: Vec::new(),
        lazies: Vec::new(),
    };
    for queue in queues {
        let roots = unsafe { &*queue }.collect_roots();
        scope.frozen.extend(roots.frozen);
        scope.cells.extend(roots.cells);
        scope.lazies.extend(roots.lazies);
    }
    with_hosts(|hosts| {
        scope
            .cells
            .extend(hosts.cells.iter().flatten().map(Rc::clone));
    });
    scope
}
