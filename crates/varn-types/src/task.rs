use crate::value::Value;
use std::cell::RefCell;

#[derive(Debug, Clone)]
pub enum TaskState {
    Pending,
    Resolved(Value),
    Rejected(Value),
}

type SettleCallback = Box<dyn FnOnce(Result<Value, Value>) + 'static>;

struct Inner {
    state: TaskState,
    on_settle: Vec<SettleCallback>,
    ref_count: u32,
}

thread_local! {
    static TASK_POOL: RefCell<Vec<*mut Inner>> = const { RefCell::new(Vec::new()) };
}

pub struct AsyncTask(*mut Inner);

impl Clone for AsyncTask {
    fn clone(&self) -> Self {
        unsafe {
            (*self.0).ref_count += 1;
        }
        AsyncTask(self.0)
    }
}

impl Drop for AsyncTask {
    fn drop(&mut self) {
        unsafe {
            (*self.0).ref_count -= 1;
            if (*self.0).ref_count == 0 {
                let inner = self.0;

                (*inner).state = TaskState::Pending;

                (*inner).on_settle.clear();

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
    fn alloc(inner: Inner) -> Self {
        let ptr = TASK_POOL.with(|pool| {
            let mut p = pool.borrow_mut();
            if let Some(ptr) = p.pop() {
                unsafe {
                    (*ptr).state = inner.state;
                    (*ptr).ref_count = 1;

                    ptr
                }
            } else {
                Box::into_raw(Box::new(inner))
            }
        });
        AsyncTask(ptr)
    }

    pub fn pending() -> Self {
        Self::alloc(Inner {
            state: TaskState::Pending,
            on_settle: Vec::new(),
            ref_count: 1,
        })
    }

    pub fn resolved(v: Value) -> Self {
        Self::alloc(Inner {
            state: TaskState::Resolved(v),
            on_settle: Vec::new(),
            ref_count: 1,
        })
    }

    pub fn rejected(v: Value) -> Self {
        Self::alloc(Inner {
            state: TaskState::Rejected(v),
            on_settle: Vec::new(),
            ref_count: 1,
        })
    }

    pub fn rejected_msg(msg: impl Into<String>) -> Self {
        let s: String = msg.into();
        Self::rejected(Value::Str(std::rc::Rc::from(s.as_str())))
    }

    pub fn peek_state(&self) -> TaskState {
        unsafe { (*self.0).state.clone() }
    }

    pub fn is_pending(&self) -> bool {
        unsafe { matches!((*self.0).state, TaskState::Pending) }
    }

    pub fn is_resolved(&self) -> bool {
        unsafe { matches!((*self.0).state, TaskState::Resolved(_)) }
    }

    pub fn is_rejected(&self) -> bool {
        unsafe { matches!((*self.0).state, TaskState::Rejected(_)) }
    }

    pub fn settle(&self, result: Result<Value, Value>) {
        let callbacks = unsafe {
            let inner = &mut *self.0;
            if !matches!(inner.state, TaskState::Pending) {
                return;
            }
            match &result {
                Ok(v) => inner.state = TaskState::Resolved(v.clone()),
                Err(v) => inner.state = TaskState::Rejected(v.clone()),
            }
            std::mem::take(&mut inner.on_settle)
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
            let inner = &mut *self.0;
            match &inner.state {
                TaskState::Pending => {
                    inner.on_settle.push(Box::new(cb));
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
