use crate::task::AsyncTask;
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub trait GeneratorDriver: std::fmt::Debug {
    fn next(&self, input: Value) -> Result<Value, String>;
    fn is_done(&self) -> bool;
    fn is_async(&self) -> bool;
    fn trace_vm_values(&self, _callback: &mut dyn FnMut(crate::VmValue)) {}
    /// Visit every mutable `VmValue` slot in the driver's suspended state
    /// (saved stack, upvalues, pending suspends) so a copying minor GC can
    /// rewrite evacuated heap indices in place. Must cover every slot
    /// `trace_vm_values` reports that can hold a nursery index.
    fn trace_vm_values_mut(&self, _callback: &mut dyn FnMut(&mut crate::VmValue)) {}
    fn trace_closures(&self, _callback: &mut dyn FnMut(usize)) {}
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
}

fn make_iter_result(value: Value, done: bool) -> Value {
    use crate::value::{value_to_nv, ObjRef};
    use crate::vm_value::VmValue;
    Value::Object(ObjRef::from_pairs([
        (Rc::from("value"), value_to_nv(&value)),
        (Rc::from("done"), VmValue::from_bool(done)),
    ]))
}
