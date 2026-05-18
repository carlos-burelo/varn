use crate::{Suspend, TaskId};
use std::rc::Rc;
use varn_types::generator::GenChannel;
use varn_types::{AsyncTask, Poll, Value};

pub trait TaskRunner {
    fn poll_vm(&mut self) -> (Poll, Option<Suspend>);
    fn push_resume_value(&mut self, r: Result<Value, Value>) -> Result<(), String>;
    fn take_pending_spawns(&mut self) -> Vec<(Box<dyn TaskRunner>, AsyncTask)>;
    fn take_pending_async_gen_spawns(&mut self) -> Vec<(TaskId, Box<dyn TaskRunner>)>;
    fn gen_channel(&self) -> Option<Rc<GenChannel>>;
}
