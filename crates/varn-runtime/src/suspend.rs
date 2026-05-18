use std::time::Duration;
use varn_types::{AsyncTask, Value};

pub enum Suspend {
    Task(AsyncTask),
    Timer(Duration),
    Yield(Value),
}
