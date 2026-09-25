mod alloc;
mod buffer;
mod class;
mod closure;
mod constructors;
mod instance;
mod map;
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
use bigdecimal::BigDecimal as Decimal;
pub use buffer::VmBuffer;
pub use class::{find_method_with_owner, ClassObj};
pub use closure::{Closure, Upvalue, UpvalueInner};
pub use constructors::{new_array, new_object};
pub use instance::{InstanceData, InstanceRef};
pub use module::{FrozenExport, FrozenModuleObj, ModuleObj};
pub use object::{nv_to_value, value_to_nv, ObjData};
pub use sendable::{HostError, SendEnumVariant, SendEnvelope, SendValue};
pub use shape::{root_shape, Shape};
use std::rc::Rc;
use std::sync::Arc;
pub use task::{reject_task, reject_value_task, resolve_task, Poll, TaskState};
pub use varn_core::{RuntimeKind, VmValuePayload};

pub type RuntimeArray = Vec<Value>;

#[derive(Debug, Clone)]
pub struct LazyTask {
    pub closure: Rc<crate::value::closure::Closure>,
    pub args: Vec<Value>,
    pub current_class: Option<Rc<crate::value::class::ClassObj>>,
}

/// The element domain of a `Range<T>`: bounds are stored as `i64` either
/// way (a `char` as its code point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RangeElem {
    Int,
    Char,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RangeData {
    pub start: i64,
    pub end: i64,
    pub inclusive: bool,
    pub step: i64,
    pub elem: RangeElem,
}

impl RangeData {
    pub fn int(start: i64, end: i64, inclusive: bool) -> Self {
        Self {
            start,
            end,
            inclusive,
            step: 1,
            elem: RangeElem::Int,
        }
    }

    pub fn end_exclusive(&self) -> i64 {
        if self.inclusive {
            self.end.saturating_add(1)
        } else {
            self.end
        }
    }

    /// Number of elements, `step` included.
    pub fn len(&self) -> i64 {
        let span = self.end_exclusive() - self.start;
        if span <= 0 {
            0
        } else {
            (span + self.step - 1) / self.step
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The raw bound of element `i`, or `None` past the end.
    pub fn nth(&self, i: i64) -> Option<i64> {
        (0..self.len())
            .contains(&i)
            .then(|| self.start + i * self.step)
    }

    pub fn contains(&self, raw: i64) -> bool {
        raw >= self.start && raw < self.end_exclusive() && (raw - self.start) % self.step == 0
    }

    /// The element `raw` stands for.
    pub fn element(&self, raw: i64) -> crate::Value {
        match self.elem {
            RangeElem::Int => crate::Value::Int(raw),
            RangeElem::Char => crate::Value::Char(
                u32::try_from(raw)
                    .ok()
                    .and_then(char::from_u32)
                    .unwrap_or('\0'),
            ),
        }
    }

    pub fn with_step(&self, step: i64) -> Self {
        Self {
            step,
            ..self.clone()
        }
    }
}

impl std::fmt::Display for RangeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dots = if self.inclusive { "..=" } else { ".." };
        match self.elem {
            RangeElem::Int => write!(f, "{}{dots}{}", self.start, self.end)?,
            RangeElem::Char => write!(
                f,
                "{}{dots}{}",
                self.element(self.start),
                self.element(self.end)
            )?,
        }
        if self.step != 1 {
            write!(f, " step {}", self.step)?;
        }
        Ok(())
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
    /// The enum's class (`ClassObj::id`), set when the variant is attached to
    /// it; methods and identity come from here, never from `enum_name`. `None`
    /// only for a value rebuilt on the far side of an isolate channel.
    pub enum_class_id: Option<u32>,
    pub enum_name: Arc<str>,
    pub variant_name: Arc<str>,
    pub variant_tag: i64,
    pub fields: Vec<Arc<str>>,
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
    BigInt(Box<num_bigint::BigInt>),
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
    Buffer(VmBuffer),
}

pub type ResultType = Result<Value, String>;
