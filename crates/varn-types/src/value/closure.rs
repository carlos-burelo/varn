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
    pub resolved_constants: Vec<Value>,
    pub ic_cache: Rc<Vec<std::sync::atomic::AtomicU64>>,
}

impl Closure {
    pub fn new(
        proto: Rc<FunctionProto>,
        upvalues: Vec<Upvalue>,
        resolved_constants: Vec<Value>,
    ) -> Self {
        let cache_count = proto.cache_count;
        let mut ic_cache = Vec::with_capacity(cache_count);
        for _ in 0..cache_count {
            ic_cache.push(std::sync::atomic::AtomicU64::new(0));
        }
        Self {
            proto,
            upvalues,
            resolved_constants,
            ic_cache: Rc::new(ic_cache),
        }
    }
}

use super::Value;
