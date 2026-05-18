use crate::task::AsyncTask;
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

pub trait GeneratorDriver: std::fmt::Debug {
    fn next(&self, input: Value) -> Result<Value, String>;
    fn is_done(&self) -> bool;
    fn is_async(&self) -> bool;
}

#[derive(Debug)]
pub struct GenChannel {
    pub output: RefCell<Option<AsyncTask>>,
    pub done: AtomicBool,
    pub started: AtomicBool,
    pub cancel_signal: AsyncTask,
    pub resume_value: RefCell<Option<Result<Value, Value>>>,
    pub wake_signal: RefCell<Option<AsyncTask>>,
}

impl GenChannel {
    pub fn new() -> Rc<Self> {
        Rc::new(GenChannel {
            output: RefCell::new(None),
            done: AtomicBool::new(false),
            started: AtomicBool::new(false),
            cancel_signal: AsyncTask::pending(),
            resume_value: RefCell::new(None),
            wake_signal: RefCell::new(None),
        })
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    pub fn mark_done(&self) {
        self.done.store(true, Ordering::SeqCst);
        self.cancel_signal.resolve(Value::Null);
    }
}

#[derive(Clone, Debug)]
pub struct GeneratorObj(pub Rc<dyn GeneratorDriver>);

impl PartialEq for GeneratorObj {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for GeneratorObj {}

impl std::hash::Hash for GeneratorObj {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Rc::as_ptr(&self.0) as *const () as usize).hash(state);
    }
}

#[derive(Debug)]
pub struct AsyncQueueInner {
    pub queue: std::collections::VecDeque<Value>,
    pub done: bool,
    pub waiter: Option<AsyncTask>,
}

#[derive(Clone, Debug)]
pub struct AsyncQueue(pub Rc<RefCell<AsyncQueueInner>>);

impl Default for AsyncQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncQueue {
    pub fn new() -> Self {
        AsyncQueue(Rc::new(RefCell::new(AsyncQueueInner {
            queue: std::collections::VecDeque::new(),
            done: false,
            waiter: None,
        })))
    }

    pub fn push(&self, value: Value) {
        let mut inner = self.0.borrow_mut();
        if inner.done {
            return;
        }
        if let Some(waiter) = inner.waiter.take() {
            waiter.resolve(make_iter_result(value, false));
        } else {
            inner.queue.push_back(value);
        }
    }

    pub fn close(&self) {
        let mut inner = self.0.borrow_mut();
        inner.done = true;
        if let Some(waiter) = inner.waiter.take() {
            waiter.resolve(make_iter_result(Value::Null, true));
        }
    }

    pub fn next_value(&self) -> Value {
        let mut inner = self.0.borrow_mut();
        if let Some(chunk) = inner.queue.pop_front() {
            return Value::TaskHandle(AsyncTask::resolved(make_iter_result(chunk, false)));
        }
        if inner.done {
            return Value::TaskHandle(AsyncTask::resolved(make_iter_result(Value::Null, true)));
        }
        let fut = AsyncTask::pending();
        inner.waiter = Some(fut.clone());
        Value::TaskHandle(fut)
    }
}

fn make_iter_result(value: Value, done: bool) -> Value {
    use crate::value::{value_to_nv, ObjData, ObjRef};
    use crate::vm_value::VmValue;
    let mut obj = ObjData::new();
    obj.inner.insert(Rc::from("value"), value_to_nv(&value));
    obj.inner.insert(Rc::from("done"), VmValue::from_bool(done));
    Value::Object(ObjRef::new(obj))
}
