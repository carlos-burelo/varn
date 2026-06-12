use crate::value::Value;
use std::cell::RefCell;

#[derive(Debug, Clone)]
pub enum TaskState {
    Pending,
    Resolved(Value),
    Rejected(Value),
}

type SettleCallback = Box<dyn FnOnce(Result<Value, Value>) + 'static>;

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

struct Inner {
    state: Mutex<TaskState>,
    on_settle: Mutex<Vec<SettleCallback>>,
    ref_count: AtomicU32,
}

thread_local! {
    static TASK_POOL: RefCell<Vec<*mut Inner>> = const { RefCell::new(Vec::new()) };
}

pub struct AsyncTask(*mut Inner);

unsafe impl Send for AsyncTask {}
unsafe impl Sync for AsyncTask {}

impl Clone for AsyncTask {
    fn clone(&self) -> Self {
        unsafe {
            (*self.0).ref_count.fetch_add(1, Ordering::SeqCst);
        }
        AsyncTask(self.0)
    }
}

impl Drop for AsyncTask {
    fn drop(&mut self) {
        unsafe {
            let old_ref = (*self.0).ref_count.fetch_sub(1, Ordering::SeqCst);
            if old_ref == 1 {
                let inner = self.0;

                *(*inner).state.lock().unwrap() = TaskState::Pending;
                (*inner).on_settle.lock().unwrap().clear();

                TASK_POOL.with(|pool| {
                    let mut p = pool.borrow_mut();
                    if p.len() < 1024 {
                        p.push(inner);
                    } else {
                        drop(Box::from_raw(inner));
                    }
                });
            }
        }
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
        let ptr = TASK_POOL.with(|pool| {
            let mut p = pool.borrow_mut();
            if let Some(ptr) = p.pop() {
                unsafe {
                    *(*ptr).state.lock().unwrap() = state;
                    (*ptr).ref_count.store(1, Ordering::SeqCst);
                    ptr
                }
            } else {
                Box::into_raw(Box::new(Inner {
                    state: Mutex::new(state),
                    on_settle: Mutex::new(Vec::new()),
                    ref_count: AtomicU32::new(1),
                }))
            }
        });
        AsyncTask(ptr)
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
        Self::rejected(Value::Str(std::rc::Rc::from(s.as_str())))
    }

    pub fn peek_state(&self) -> TaskState {
        unsafe { (*self.0).state.lock().unwrap().clone() }
    }

    pub fn is_pending(&self) -> bool {
        unsafe { matches!(*(*self.0).state.lock().unwrap(), TaskState::Pending) }
    }

    pub fn is_resolved(&self) -> bool {
        unsafe { matches!(*(*self.0).state.lock().unwrap(), TaskState::Resolved(_)) }
    }

    pub fn is_rejected(&self) -> bool {
        unsafe { matches!(*(*self.0).state.lock().unwrap(), TaskState::Rejected(_)) }
    }

    pub fn settle(&self, result: Result<Value, Value>) {
        let callbacks = unsafe {
            let inner = &*self.0;
            let mut state_guard = inner.state.lock().unwrap();
            if !matches!(*state_guard, TaskState::Pending) {
                return;
            }
            match &result {
                Ok(v) => *state_guard = TaskState::Resolved(v.clone()),
                Err(v) => *state_guard = TaskState::Rejected(v.clone()),
            }
            std::mem::take(&mut *inner.on_settle.lock().unwrap())
        };
        for cb in callbacks {
            cb(result.clone());
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
        self.reject(Value::Str(std::rc::Rc::from(s.as_str())));
    }

    pub fn on_settle<F>(&self, cb: F)
    where
        F: FnOnce(Result<Value, Value>) + 'static,
    {
        let already = unsafe {
            let inner = &*self.0;
            let state_guard = inner.state.lock().unwrap();
            match &*state_guard {
                TaskState::Pending => {
                    inner.on_settle.lock().unwrap().push(Box::new(cb));
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

    #[inline]
    pub fn ptr_key(&self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub fn rejected_value(v: Value) -> Self {
        Self::rejected(v)
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
