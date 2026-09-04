mod alloc;
mod buffer;
mod class;
mod closure;
mod constructors;
mod module;
mod object;
mod sendable;
mod shape;
mod task;
mod traits;
use crate::generator::GeneratorObj;
pub use crate::native::NativeFn;
use crate::task::AsyncTask;
pub use alloc::{
    alloc_array, alloc_map, alloc_object, alloc_set, get_global_vtable, init_thread_heap,
    install_allocator, register_global_vtable, AllocVtable, ArrayRef, MapKey, MapRef, ObjRef,
    RuntimeString, SetRef, ValueMap, ValueSet,
};
pub use buffer::VmBuffer;
pub use class::{find_method_with_owner, ClassObj};
pub use closure::{Closure, Upvalue, UpvalueInner};
pub use constructors::{new_array, new_object};
pub use module::{FrozenExport, FrozenModuleObj, ModuleObj};
pub use object::{nv_to_value, value_to_nv, InstanceData, InstanceRef, ObjData};
use rust_decimal::Decimal;
pub use sendable::{HostError, SendEnumVariant, SendEnvelope, SendValue};
pub use shape::{root_shape, Shape};
use std::rc::Rc;
pub use task::{reject_task, reject_value_task, resolve_task, Poll, TaskState};
pub use varn_core::{TypeTag, VmValuePayload};

pub type RuntimeArray = Vec<Value>;

#[derive(Debug, Clone)]
pub struct LazyTask {
    pub closure: Rc<crate::value::closure::Closure>,
    pub args: Vec<Value>,
    pub current_class: Option<Rc<crate::value::class::ClassObj>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeData {
    pub start: i64,
    pub end: i64,
    pub inclusive: bool,
    pub step: i64,
}

impl std::hash::Hash for RangeData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.start.hash(state);
        self.end.hash(state);
        self.inclusive.hash(state);
        self.step.hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RuntimeSymbol {
    Iterator,
    AsyncIterator,
}

impl RuntimeSymbol {
    pub fn name(&self) -> &'static str {
        match self {
            RuntimeSymbol::Iterator => "Symbol.iterator",
            RuntimeSymbol::AsyncIterator => "Symbol.asyncIterator",
        }
    }
}

impl std::fmt::Display for RuntimeSymbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone)]
pub struct EnumVariantData {
    pub enum_name: Rc<str>,
    pub variant_name: Rc<str>,
    pub variant_tag: i64,
    pub fields: Vec<Rc<str>>,
    pub payload: Value,
}

#[derive(Clone, Debug)]
pub enum BoundMethodTarget {
    Native {
        func: NativeFn,
        name: &'static str,
    },
    Vm {
        closure: Box<dyn VmValuePayload>,
        owner_class: Option<Rc<ClassObj>>,
    },
}

#[derive(Clone, Debug)]
pub struct BoundMethod {
    pub receiver: Value,
    pub target: BoundMethodTarget,
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(RuntimeString),
    BigInt(Box<i128>),
    Decimal(Box<Decimal>),
    Array(ArrayRef),
    Object(ObjRef),
    Class(Rc<ClassObj>),
    NativeFn(Box<(NativeFn, &'static str)>),
    BoundMethod(Box<BoundMethod>),
    Spread(Box<Value>),
    Task(Rc<LazyTask>),
    TaskHandle(AsyncTask),
    Range(Box<RangeData>),
    Map(MapRef),
    Set(SetRef),
    Symbol(RuntimeSymbol),
    Generator(GeneratorObj),
    Char(char),
    EnumVariant(Box<EnumVariantData>),
    VmValue(Box<dyn VmValuePayload>),
    Module(Rc<ModuleObj>),
}

pub type ResultType = Result<Value, String>;
