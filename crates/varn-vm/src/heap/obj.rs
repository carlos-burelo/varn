use super::str::HeapStr;
use crate::closure::VmClosure;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::Arc;
use varn_base::VmValuePayload;
use varn_types::{
    generator::{AsyncQueue, GeneratorObj},
    value::{
        BoundMethod, EnumVariantData, FrozenModuleObj, MapRef, ModuleObj, ObjRef, RangeData,
        RuntimeSymbol, SetRef,
    },
    AsyncTask, ClassObj, LazyTask, NativeFn, VmArray,
};

// `repr(u8)` pins the discriminant to the first byte with a defined layout
// (RFC 2195), so JIT code can type-check a heap slot with one byte load.
#[derive(Debug, Clone)]
#[repr(u8)]
pub enum HeapObj {
    Str(HeapStr),
    Array(VmArray),
    Tuple(VmArray),
    Object(ObjRef),
    Record(ObjRef),
    Buffer(varn_types::VmBuffer),

    Module(Rc<ModuleObj>),

    FrozenModule(Arc<FrozenModuleObj>),
    VmClosure(Rc<VmClosure>),
    Class(Rc<ClassObj>),
    NativeFn(&'static str, NativeFn),
    BoundMethod(Box<BoundMethod>),
    Map(MapRef),
    Set(SetRef),
    Task(Rc<LazyTask>),
    TaskHandle(AsyncTask),
    Range(RangeData),
    Symbol(RuntimeSymbol),
    EnumVariant(Box<EnumVariantData>),
    BigInt(i128),
    Decimal(Box<rust_decimal::Decimal>),
    Char(char),
    Generator(GeneratorObj),
    AsyncQueue(AsyncQueue),
    Spread(VmValue),
    VmValue(Box<dyn VmValuePayload>),
}

impl HeapObj {
    /// The single canonical [`TypeTag`] of this heap object. Callables
    /// (closure / native fn / bound method) coalesce to `Function`; modules
    /// present as `Object`; spreads as `Array`; opaque host payloads as `VmRef`.
    /// All value-kind name rendering flows through this — see [`TypeTag::name`].
    pub(crate) fn tag(&self) -> varn_base::TypeTag {
        use varn_base::TypeTag;
        match self {
            HeapObj::Str(_) => TypeTag::Str,
            HeapObj::Array(_) | HeapObj::Tuple(_) => TypeTag::Array,
            HeapObj::Object(_)
            | HeapObj::Record(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_) => TypeTag::Object,
            HeapObj::VmClosure(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) => {
                TypeTag::Function
            }
            HeapObj::Class(_) => TypeTag::Class,
            HeapObj::Map(_) => TypeTag::Map,
            HeapObj::Set(_) => TypeTag::Set,
            HeapObj::Task(_) => TypeTag::Task,
            HeapObj::TaskHandle(_) => TypeTag::TaskHandle,
            HeapObj::Range(_) => TypeTag::Range,
            HeapObj::Symbol(_) => TypeTag::Symbol,
            HeapObj::EnumVariant(_) => TypeTag::Enum,
            HeapObj::BigInt(_) => TypeTag::BigInt,
            HeapObj::Decimal(_) => TypeTag::Decimal,
            HeapObj::Char(_) => TypeTag::Char,
            HeapObj::Generator(_) => TypeTag::Generator,
            HeapObj::AsyncQueue(_) => TypeTag::AsyncQueue,
            HeapObj::Spread(_) => TypeTag::Array,
            HeapObj::Buffer(_) | HeapObj::VmValue(_) => TypeTag::VmRef,
        }
    }
}
