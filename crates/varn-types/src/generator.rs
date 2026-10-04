use crate::vm_value::VmValue;
use std::rc::Rc;

pub trait GeneratorDriver: std::fmt::Debug {
    fn next(&self, input: VmValue) -> Result<VmValue, String>;
    fn is_done(&self) -> bool;
    fn is_async(&self) -> bool;
    fn trace_vm_values(&self, _callback: &mut dyn FnMut(crate::VmValue)) {}
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
