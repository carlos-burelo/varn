use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use varn_types::{ClassObj, FunctionProto, HostOpen, HostPromise};

use crate::heap::{HeapInner, HeapObj};
use crate::value::VmValue;

const INLINE_ARGS: usize = 3;

pub(crate) type Outcome = Result<VmValue, VmValue>;
pub(crate) type Inbox = Rc<RefCell<VecDeque<u64>>>;

pub(crate) enum TaskArgs {
    Inline(u8, [Cell<VmValue>; INLINE_ARGS]),
    Heap(Vec<Cell<VmValue>>),
}

impl TaskArgs {
    pub(crate) fn collect(mut values: impl ExactSizeIterator<Item = VmValue>) -> Self {
        let len = values.len();
        if len > INLINE_ARGS {
            return TaskArgs::Heap(values.map(Cell::new).collect());
        }
        let inline = [
            Cell::new(VmValue::null()),
            Cell::new(VmValue::null()),
            Cell::new(VmValue::null()),
        ];
        for slot in inline.iter().take(len) {
            if let Some(v) = values.next() {
                slot.set(v);
            }
        }
        TaskArgs::Inline(len as u8, inline)
    }

    pub(crate) fn cells(&self) -> &[Cell<VmValue>] {
        match self {
            TaskArgs::Inline(len, inline) => &inline[..*len as usize],
            TaskArgs::Heap(values) => values,
        }
    }
}

pub struct LazyTask {
    pub(crate) proto: Rc<FunctionProto>,
    pub(crate) upvalues: Vec<Cell<VmValue>>,
    pub(crate) module_base: u32,
    pub(crate) args: TaskArgs,
    pub(crate) current_class: Option<Rc<ClassObj>>,
}

impl std::fmt::Debug for LazyTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "LazyTask({})",
            self.proto.name.as_deref().unwrap_or("<anon>")
        )
    }
}

impl LazyTask {
    pub(crate) fn trace_cells(&self, f: &mut dyn FnMut(&Cell<VmValue>)) {
        for cell in self.args.cells() {
            f(cell);
        }
        for cell in &self.upvalues {
            f(cell);
        }
    }

    fn holds_young_ref(&self, heap: &HeapInner) -> bool {
        let mut found = false;
        self.trace_cells(&mut |cell| found |= heap.is_young(cell.get()));
        found
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Status {
    Pending,
    Resolved,
    Rejected,
}

pub(crate) enum Waiter {
    Resume { inbox: Inbox, token: u64 },
    Gather { parent: Rc<TaskCell>, index: u32 },
}

#[derive(Default)]
struct Waiters {
    first: Option<Waiter>,
    more: Option<Box<Vec<Waiter>>>,
}

impl Waiters {
    fn push(&mut self, waiter: Waiter) {
        if self.first.is_none() {
            self.first = Some(waiter);
            return;
        }
        self.more.get_or_insert_with(Default::default).push(waiter);
    }

    fn drain(self) -> impl Iterator<Item = Waiter> {
        self.first
            .into_iter()
            .chain(self.more.into_iter().flat_map(|more| *more))
    }
}

struct Gather {
    remaining: Cell<usize>,
    has_error: Cell<bool>,
    error: Cell<VmValue>,
}

struct Extra {
    host: RefCell<Option<(HostPromise, HostOpen)>>,
    gather: Option<Gather>,
}

pub struct TaskCell {
    status: Cell<Status>,
    yielded: bool,
    value: Cell<VmValue>,
    waiters: RefCell<Waiters>,
    extra: Option<Box<Extra>>,
}

impl std::fmt::Debug for TaskCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TaskCell({:?})", self.status.get())
    }
}

impl TaskCell {
    fn new(yielded: bool, extra: Option<Extra>, value: VmValue) -> Self {
        Self {
            status: Cell::new(Status::Pending),
            yielded,
            value: Cell::new(value),
            waiters: RefCell::new(Waiters::default()),
            extra: extra.map(Box::new),
        }
    }

    pub(crate) fn pending() -> Rc<Self> {
        Rc::new(Self::new(false, None, VmValue::null()))
    }

    pub(crate) fn yielded() -> Rc<Self> {
        Rc::new(Self::new(true, None, VmValue::null()))
    }

    pub(crate) fn host(promise: HostPromise, open: HostOpen) -> Rc<Self> {
        Rc::new(Self::new(
            false,
            Some(Extra {
                host: RefCell::new(Some((promise, open))),
                gather: None,
            }),
            VmValue::null(),
        ))
    }

    pub(crate) fn gather(results: VmValue, count: usize) -> Rc<Self> {
        Rc::new(Self::new(
            false,
            Some(Extra {
                host: RefCell::new(None),
                gather: Some(Gather {
                    remaining: Cell::new(count),
                    has_error: Cell::new(false),
                    error: Cell::new(VmValue::null()),
                }),
            }),
            results,
        ))
    }

    fn gather_state(&self) -> Option<&Gather> {
        self.extra.as_ref().and_then(|extra| extra.gather.as_ref())
    }

    #[inline]
    pub(crate) fn status(&self) -> Status {
        self.status.get()
    }

    #[inline]
    pub(crate) fn is_pending(&self) -> bool {
        self.status.get() == Status::Pending
    }

    #[inline]
    pub(crate) fn is_yield(&self) -> bool {
        self.yielded
    }

    #[inline]
    pub(crate) fn value(&self) -> VmValue {
        self.value.get()
    }

    pub(crate) fn take_host(&self) -> Option<(HostPromise, HostOpen)> {
        self.extra.as_ref()?.host.borrow_mut().take()
    }

    pub(crate) fn host_promise(&self) -> Option<HostPromise> {
        let extra = self.extra.as_ref()?;
        let host = extra.host.borrow();
        host.as_ref().map(|(promise, _)| promise.clone())
    }

    pub(crate) fn watch(&self, inbox: &Inbox, token: u64) {
        if self.is_pending() {
            self.waiters.borrow_mut().push(Waiter::Resume {
                inbox: Rc::clone(inbox),
                token,
            });
        } else {
            inbox.borrow_mut().push_back(token);
        }
    }

    pub(crate) fn gather_into(self: &Rc<Self>, parent: &Rc<TaskCell>, index: u32) -> bool {
        if self.is_pending() {
            self.waiters.borrow_mut().push(Waiter::Gather {
                parent: Rc::clone(parent),
                index,
            });
            return true;
        }
        false
    }

    pub(crate) fn trace_cells(&self, f: &mut dyn FnMut(&Cell<VmValue>)) {
        f(&self.value);
        if let Some(gather) = self.gather_state() {
            f(&gather.error);
        }
    }
}

pub(crate) fn track_cell(heap: &mut HeapInner, cell: &Rc<TaskCell>) {
    let mut young = false;
    cell.trace_cells(&mut |c| young |= heap.is_young(c.get()));
    if young {
        heap.young_cells.push(Rc::clone(cell));
    }
}

pub(crate) fn track_lazy(heap: &mut HeapInner, lazy: &Rc<LazyTask>) {
    if lazy.holds_young_ref(heap) {
        heap.young_lazies.push(Rc::clone(lazy));
    }
}

pub(crate) fn settle(heap: &mut HeapInner, cell: &Rc<TaskCell>, outcome: Outcome) {
    if cell.status.get() != Status::Pending {
        return;
    }
    let (status, value) = match outcome {
        Ok(v) => (Status::Resolved, v),
        Err(e) => (Status::Rejected, e),
    };
    cell.status.set(status);
    cell.value.set(value);
    track_cell(heap, cell);
    let waiters = std::mem::take(&mut *cell.waiters.borrow_mut());
    for waiter in waiters.drain() {
        match waiter {
            Waiter::Resume { inbox, token } => inbox.borrow_mut().push_back(token),
            Waiter::Gather { parent, index } => gather_one(heap, &parent, index, outcome_of(cell)),
        }
    }
}

fn outcome_of(cell: &TaskCell) -> Outcome {
    match cell.status.get() {
        Status::Resolved => Ok(cell.value.get()),
        Status::Rejected | Status::Pending => Err(cell.value.get()),
    }
}

pub(crate) fn gather_one(
    heap: &mut HeapInner,
    parent: &Rc<TaskCell>,
    index: u32,
    outcome: Outcome,
) {
    let Some(gather) = parent.gather_state() else {
        return;
    };
    match outcome {
        Ok(value) => {
            let results = parent.value.get();
            if results.is_heap() {
                let idx = results.as_heap_idx();
                let array = match heap.get(idx) {
                    Some(HeapObj::Array(a)) => Some(a.clone()),
                    _ => None,
                };
                if let Some(array) = array {
                    array.set_vm(index as usize, value);
                    heap.write_barrier(idx, value);
                }
            }
        }
        Err(error) => {
            if !gather.has_error.get() {
                gather.has_error.set(true);
                gather.error.set(error);
                track_cell(heap, parent);
            }
        }
    }
    let remaining = gather.remaining.get().saturating_sub(1);
    gather.remaining.set(remaining);
    if remaining == 0 {
        let final_outcome = if gather.has_error.get() {
            Err(gather.error.get())
        } else {
            Ok(parent.value.get())
        };
        settle(heap, parent, final_outcome);
    }
}

pub(crate) fn new_lazy(
    heap: &mut HeapInner,
    proto: Rc<FunctionProto>,
    upvalues: Vec<VmValue>,
    module_base: u32,
    args: impl ExactSizeIterator<Item = VmValue>,
    current_class: Option<Rc<ClassObj>>,
) -> VmValue {
    let lazy = Rc::new(LazyTask {
        proto,
        upvalues: upvalues.into_iter().map(Cell::new).collect(),
        module_base,
        args: TaskArgs::collect(args),
        current_class,
    });
    track_lazy(heap, &lazy);
    VmValue::from_heap_idx(heap.alloc(HeapObj::Task(lazy)))
}

pub(crate) fn alloc_handle(heap: &mut HeapInner, cell: Rc<TaskCell>) -> VmValue {
    VmValue::from_heap_idx(heap.alloc(HeapObj::TaskHandle(cell)))
}

pub(crate) fn outcome_of_cell(cell: &TaskCell) -> Outcome {
    outcome_of(cell)
}
