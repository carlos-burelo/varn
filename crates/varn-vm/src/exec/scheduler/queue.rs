use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use super::suspend::Frozen;
use crate::exec::ctx::ExecCtx;
use crate::task::{Inbox, LazyTask, TaskCell};

const SPARE_CTX_LIMIT: usize = 8;
const SWEEP_FLOOR: usize = 1024;

pub(crate) enum Delivery {
    Nothing,
    Settled(Rc<TaskCell>),
}

pub(crate) enum Start {
    Fresh(Rc<LazyTask>),
    Resume(Box<Frozen>, Delivery),
}

pub(crate) struct ReadyTask {
    pub(crate) start: Start,
    pub(crate) output: Rc<TaskCell>,
}

struct ParkedTask {
    output: Rc<TaskCell>,
    waiting_on: Rc<TaskCell>,
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
    #[allow(clippy::vec_box)]
    spare: Vec<Box<ExecCtx>>,
}

pub(crate) struct TaskQueue {
    local: RefCell<Local>,
    inbox: Inbox,
}

fn token(index: u32, generation: u32) -> u64 {
    (u64::from(generation) << 32) | u64::from(index)
}

pub(crate) struct QueueRoots {
    pub(crate) frozen: Vec<*mut Frozen>,
    pub(crate) cells: Vec<Rc<TaskCell>>,
    pub(crate) lazies: Vec<Rc<LazyTask>>,
}

impl TaskQueue {
    pub(crate) fn new() -> Self {
        Self {
            local: RefCell::new(Local::default()),
            inbox: Inbox::default(),
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

    pub(crate) fn has_inbox(&self) -> bool {
        !self.inbox.borrow().is_empty()
    }

    pub(crate) fn park(&self, output: Rc<TaskCell>, waiting_on: Rc<TaskCell>, frozen: Box<Frozen>) {
        let (index, generation) = self.local.borrow_mut().parked.insert(ParkedTask {
            output,
            waiting_on: Rc::clone(&waiting_on),
            frozen,
        });
        waiting_on.watch(&self.inbox, token(index, generation));
    }

    pub(crate) fn collect_resumes(&self) {
        loop {
            let Some(raw) = self.inbox.borrow_mut().pop_front() else {
                return;
            };
            let index = (raw & 0xFFFF_FFFF) as u32;
            let generation = (raw >> 32) as u32;
            let Some(parked) = self.local.borrow_mut().parked.remove(index, generation) else {
                continue;
            };
            if !parked.output.is_pending() {
                continue;
            }
            if parked.waiting_on.is_pending() {
                self.park(parked.output, parked.waiting_on, parked.frozen);
                continue;
            }
            self.push_ready(ReadyTask {
                start: Start::Resume(parked.frozen, Delivery::Settled(parked.waiting_on)),
                output: parked.output,
            });
        }
    }

    pub(crate) fn collect_roots(&self) -> QueueRoots {
        let mut guard = self.local.borrow_mut();
        let local = &mut *guard;
        let mut roots = QueueRoots {
            frozen: Vec::new(),
            cells: Vec::new(),
            lazies: Vec::new(),
        };
        for ready in local.ready.iter_mut() {
            roots.cells.push(Rc::clone(&ready.output));
            match &mut ready.start {
                Start::Fresh(lazy) => roots.lazies.push(Rc::clone(lazy)),
                Start::Resume(frozen, delivery) => {
                    roots.frozen.push(&mut **frozen as *mut Frozen);
                    if let Delivery::Settled(cell) = delivery {
                        roots.cells.push(Rc::clone(cell));
                    }
                }
            }
        }
        for cell in local.parked.cells.iter_mut() {
            if let Some(parked) = cell.task.as_mut() {
                roots.frozen.push(&mut *parked.frozen as *mut Frozen);
                roots.cells.push(Rc::clone(&parked.output));
                roots.cells.push(Rc::clone(&parked.waiting_on));
            }
        }
        roots
    }
}
