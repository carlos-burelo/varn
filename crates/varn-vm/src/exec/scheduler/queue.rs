use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use varn_types::value::LazyTask;
use varn_types::{AsyncTask, TaskState, Value, WakeQueue, WakeToken};

use super::suspend::Frozen;
use crate::exec::ctx::ExecCtx;

const SPARE_CTX_LIMIT: usize = 8;
const KICK: u64 = u64::MAX;
const SWEEP_FLOOR: usize = 1024;

pub(crate) enum Delivery {
    Nothing,
    Resolved(Value),
    Rejected(Value),
}

pub(crate) enum Start {
    Fresh(Rc<LazyTask>),
    Resume(Box<Frozen>, Delivery),
}

pub(crate) struct ReadyTask {
    pub(crate) start: Start,
    pub(crate) output: AsyncTask,
}

struct ParkedTask {
    output: AsyncTask,
    waiting_on: AsyncTask,
    frozen: Box<Frozen>,
}

struct Cell {
    generation: u32,
    task: Option<ParkedTask>,
}

struct Parked {
    cells: Vec<Cell>,
    free: Vec<u32>,
    live: usize,
    sweep_at: usize,
}

impl Default for Parked {
    fn default() -> Self {
        Self {
            cells: Vec::new(),
            free: Vec::new(),
            live: 0,
            sweep_at: SWEEP_FLOOR,
        }
    }
}

impl Parked {
    fn sweep_abandoned(&mut self) {
        for (index, cell) in self.cells.iter_mut().enumerate() {
            let abandoned = cell
                .task
                .as_ref()
                .is_some_and(|parked| !parked.output.is_pending());
            if abandoned {
                cell.task = None;
                cell.generation = cell.generation.wrapping_add(1);
                self.free.push(index as u32);
                self.live -= 1;
            }
        }
        self.sweep_at = SWEEP_FLOOR.max(self.live * 2);
    }

    fn insert(&mut self, task: ParkedTask) -> (u32, u32) {
        if self.live >= self.sweep_at {
            self.sweep_abandoned();
        }
        self.live += 1;
        match self.free.pop() {
            Some(index) => {
                let cell = &mut self.cells[index as usize];
                cell.task = Some(task);
                (index, cell.generation)
            }
            None => {
                self.cells.push(Cell {
                    generation: 0,
                    task: Some(task),
                });
                (self.cells.len() as u32 - 1, 0)
            }
        }
    }

    fn remove(&mut self, index: u32, generation: u32) -> Option<ParkedTask> {
        let cell = self.cells.get_mut(index as usize)?;
        if cell.generation != generation {
            return None;
        }
        let task = cell.task.take()?;
        cell.generation = cell.generation.wrapping_add(1);
        self.free.push(index);
        self.live -= 1;
        Some(task)
    }
}

#[derive(Default)]
struct Local {
    ready: VecDeque<ReadyTask>,
    parked: Parked,
    incoming: VecDeque<u64>,
    #[allow(clippy::vec_box)]
    spare: Vec<Box<ExecCtx>>,
}

pub(crate) struct TaskQueue {
    local: RefCell<Local>,
    wakes: Arc<WakeQueue>,
}

fn token(index: u32, generation: u32) -> u64 {
    (u64::from(generation) << 32) | u64::from(index)
}

impl TaskQueue {
    pub(crate) fn new() -> Self {
        Self {
            local: RefCell::new(Local::default()),
            wakes: WakeQueue::new(),
        }
    }

    pub(crate) fn push_ready(&self, task: ReadyTask) {
        self.local.borrow_mut().ready.push_back(task);
    }

    pub(crate) fn pop_ready(&self) -> Option<ReadyTask> {
        self.local.borrow_mut().ready.pop_front()
    }

    pub(crate) fn take_spare(&self) -> Option<Box<ExecCtx>> {
        self.local.borrow_mut().spare.pop()
    }

    pub(crate) fn give_spare(&self, ctx: Box<ExecCtx>) {
        let mut local = self.local.borrow_mut();
        if local.spare.len() < SPARE_CTX_LIMIT {
            local.spare.push(ctx);
        }
    }

    pub(crate) fn wake_generation(&self) -> u64 {
        self.wakes.generation()
    }

    pub(crate) fn wait_for_change(&self, entered: u64, done: impl Fn() -> bool) {
        self.wakes.wait_past(entered, done);
    }

    pub(crate) fn kick_on_settle(&self, handle: &AsyncTask) {
        handle.wake_on_settle(WakeToken::new(&self.wakes, KICK));
    }

    pub(crate) fn park(&self, output: AsyncTask, waiting_on: AsyncTask, frozen: Box<Frozen>) {
        let (index, generation) = self.local.borrow_mut().parked.insert(ParkedTask {
            output,
            waiting_on: waiting_on.clone(),
            frozen,
        });
        waiting_on.wake_on_settle(WakeToken::new(&self.wakes, token(index, generation)));
    }

    pub(crate) fn wake_settled(&self) {
        let mut incoming = std::mem::take(&mut self.local.borrow_mut().incoming);
        self.wakes.drain_into(&mut incoming);
        while let Some(raw) = incoming.pop_front() {
            if raw == KICK {
                continue;
            }
            let index = (raw & 0xFFFF_FFFF) as u32;
            let generation = (raw >> 32) as u32;
            let Some(parked) = self.local.borrow_mut().parked.remove(index, generation) else {
                continue;
            };
            if !parked.output.is_pending() {
                continue;
            }
            let delivery = match parked.waiting_on.peek_state() {
                TaskState::Pending => {
                    self.repark(parked);
                    continue;
                }
                TaskState::Resolved(v) => Delivery::Resolved(v),
                TaskState::Rejected(e) => Delivery::Rejected(e),
            };
            self.push_ready(ReadyTask {
                start: Start::Resume(parked.frozen, delivery),
                output: parked.output,
            });
        }
        self.local.borrow_mut().incoming = incoming;
    }

    fn repark(&self, parked: ParkedTask) {
        self.park(parked.output, parked.waiting_on, parked.frozen);
    }

    pub(crate) fn for_each_frozen(&self, mut f: impl FnMut(*mut Frozen)) {
        let mut guard = self.local.borrow_mut();
        let local = &mut *guard;
        for ready in local.ready.iter_mut() {
            if let Start::Resume(frozen, _) = &mut ready.start {
                f(&mut **frozen as *mut Frozen);
            }
        }
        for cell in local.parked.cells.iter_mut() {
            if let Some(parked) = cell.task.as_mut() {
                f(&mut *parked.frozen as *mut Frozen);
            }
        }
    }
}
