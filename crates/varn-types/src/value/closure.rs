use crate::chunk::FunctionProto;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct Upvalue {
    pub inner: Rc<RefCell<UpvalueInner>>,
}

#[derive(Debug, Clone)]
pub struct UpvalueInner {
    pub value: Value,
    pub location: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Closure {
    pub proto: Rc<FunctionProto>,
    pub upvalues: Vec<Upvalue>,
    /// Global-region base of the module this closure belongs to, carried across
    /// a task/isolate fork so the forked frame resolves `LoadGlobalIdx` against
    /// the right region. `0` for a closure with no enclosing module region.
    pub module_base: u32,
}

impl Closure {
    pub fn new(proto: Rc<FunctionProto>, upvalues: Vec<Upvalue>) -> Self {
        Self::with_module_base(proto, upvalues, 0)
    }

    pub fn with_module_base(
        proto: Rc<FunctionProto>,
        upvalues: Vec<Upvalue>,
        module_base: u32,
    ) -> Self {
        Self {
            proto,
            upvalues,
            module_base,
        }
    }
}

use super::Value;
