pub use crate::task::{Poll, TaskState};

use super::Value;
use crate::task::AsyncTask;

pub fn resolve_task(v: Value) -> Value {
    Value::TaskHandle(AsyncTask::resolved(v))
}

pub fn reject_task(msg: String) -> Value {
    Value::TaskHandle(AsyncTask::rejected_msg(msg))
}

pub fn reject_value_task(v: Value) -> Value {
    Value::TaskHandle(AsyncTask::rejected(v))
}
