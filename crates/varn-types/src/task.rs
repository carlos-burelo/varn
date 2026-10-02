use crate::value::Value;
#[derive(Debug, Clone)]
pub enum TaskState {
    Pending,
    Resolved(Value),
    Rejected(Value),
}

type SettleCallback = Box<dyn FnOnce(Result<Value, Value>) + 'static>;

use crate::wake::WakeToken;
use std::sync::atomic::{fence, AtomicU32, Ordering};
use std::sync::Mutex;

enum Waiter {
    Wake(WakeToken),
    Call(SettleCallback),
}

struct Slot {
    state: TaskState,
    first: Option<Waiter>,
    more: Vec<Waiter>,
}

impl Slot {
    fn push(&mut self, waiter: Waiter) {
        if self.first.is_none() {
            self.first = Some(waiter);
            return;
        }
        self.more.reserve_exact(1);
        self.more.push(waiter);
    }

    fn take_waiters(&mut self) -> (Option<Waiter>, Vec<Waiter>) {
        (self.first.take(), std::mem::take(&mut self.more))
    }
}

struct Inner {
    slot: Mutex<Slot>,
    ref_count: AtomicU32,
}

pub struct AsyncTask(*mut Inner);

unsafe impl Send for AsyncTask {}
unsafe impl Sync for AsyncTask {}

impl Clone for AsyncTask {
    fn clone(&self) -> Self {
        unsafe {
            (*self.0).ref_count.fetch_add(1, Ordering::Relaxed);
        }
        AsyncTask(self.0)
    }
}

impl Drop for AsyncTask {
    fn drop(&mut self) {
        unsafe {
            if (*self.0).ref_count.fetch_sub(1, Ordering::Release) == 1 {
                fence(Ordering::Acquire);
                drop(Box::from_raw(self.0));
            }
        }
    }
}

impl AsyncTask {
    /// Stable identity of this task while any clone is alive (the `Inner`
    /// pointer). Only meaningful as a map key alongside an owning clone: the
    /// allocation is reused once every clone drops.
    #[inline(always)]
    pub fn identity(&self) -> usize {
        self.0 as usize
    }
}

impl PartialEq for AsyncTask {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for AsyncTask {}

impl std::hash::Hash for AsyncTask {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl std::fmt::Debug for AsyncTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.peek_state() {
            TaskState::Pending => write!(f, "Task(<pending>)"),
            TaskState::Resolved(v) => write!(f, "Task({v})"),
            TaskState::Rejected(v) => write!(f, "Task(<rejected:{v}>)"),
        }
    }
}

impl AsyncTask {
    fn alloc(state: TaskState) -> Self {
        let inner = Box::new(Inner {
            slot: Mutex::new(Slot {
                state,
                first: None,
                more: Vec::new(),
            }),
            ref_count: AtomicU32::new(1),
        });
        AsyncTask(Box::into_raw(inner))
    }

    #[inline(always)]
    fn slot(&self) -> std::sync::MutexGuard<'_, Slot> {
        unsafe { (*self.0).slot.lock().unwrap() }
    }

    pub fn pending() -> Self {
        Self::alloc(TaskState::Pending)
    }

    pub fn resolved(v: Value) -> Self {
        Self::alloc(TaskState::Resolved(v))
    }

    pub fn rejected(v: Value) -> Self {
        Self::alloc(TaskState::Rejected(v))
    }

    pub fn rejected_msg(msg: impl Into<String>) -> Self {
        let s: String = msg.into();
        Self::rejected(Value::Str(std::sync::Arc::from(s.as_str())))
    }

    #[inline]
    pub fn is_pending(&self) -> bool {
        matches!(self.slot().state, TaskState::Pending)
    }

    pub fn peek_state(&self) -> TaskState {
        self.slot().state.clone()
    }

    pub fn settle(&self, result: Result<Value, Value>) {
        let waiters = {
            let mut slot = self.slot();
            if !matches!(slot.state, TaskState::Pending) {
                return;
            }
            slot.state = match &result {
                Ok(v) => TaskState::Resolved(v.clone()),
                Err(v) => TaskState::Rejected(v.clone()),
            };
            slot.take_waiters()
        };
        let (first, more) = waiters;
        for waiter in first.into_iter().chain(more) {
            match waiter {
                Waiter::Wake(token) => token.fire(),
                Waiter::Call(cb) => cb(result.clone()),
            }
        }
    }

    #[inline]
    pub fn resolve(&self, v: Value) {
        self.settle(Ok(v));
    }

    #[inline]
    pub fn reject(&self, v: Value) {
        self.settle(Err(v));
    }

    #[inline]
    pub fn reject_msg(&self, msg: impl Into<String>) {
        let s: String = msg.into();
        self.reject(Value::Str(std::sync::Arc::from(s.as_str())));
    }

    pub fn wake_on_settle(&self, token: WakeToken) {
        {
            let mut slot = self.slot();
            if matches!(slot.state, TaskState::Pending) {
                slot.push(Waiter::Wake(token));
                return;
            }
        }
        token.fire();
    }

    pub fn on_settle<F>(&self, cb: F)
    where
        F: FnOnce(Result<Value, Value>) + 'static,
    {
        let already = {
            let mut slot = self.slot();
            match &slot.state {
                TaskState::Pending => {
                    slot.push(Waiter::Call(Box::new(cb)));
                    return;
                }
                TaskState::Resolved(v) => Ok(v.clone()),
                TaskState::Rejected(v) => Err(v.clone()),
            }
        };
        cb(already);
    }

    #[inline]
    pub fn cancel(&self) {
        self.reject_msg("Task cancelled");
    }
}

static YIELD_TOKEN: std::sync::OnceLock<AsyncTask> = std::sync::OnceLock::new();

impl AsyncTask {
    pub fn yield_token() -> Self {
        YIELD_TOKEN.get_or_init(AsyncTask::pending).clone()
    }

    pub fn is_yield_token(&self) -> bool {
        let token = YIELD_TOKEN.get_or_init(AsyncTask::pending);
        self.identity() == token.identity()
    }
}

pub enum Poll {
    Ready(Result<Value, String>),
    Pending,
}

pub fn resolve_task(v: Value) -> Value {
    Value::TaskHandle(AsyncTask::resolved(v))
}

pub fn reject_task(msg: String) -> Value {
    Value::TaskHandle(AsyncTask::rejected_msg(msg))
}

pub fn reject_value_task(v: Value) -> Value {
    Value::TaskHandle(AsyncTask::rejected(v))
}
