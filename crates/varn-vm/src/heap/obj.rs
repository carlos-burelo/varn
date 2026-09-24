use super::str::HeapStr;
use crate::closure::VmClosure;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::Arc;
use varn_core::VmValuePayload;
use varn_types::{
    generator::GeneratorObj,
    value::{
        BoundMethod, EnumVariantData, FrozenModuleObj, InstanceRef, MapRef, ModuleObj, ObjRef,
        RangeData, RuntimeSymbol, SetRef,
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
    Instance(InstanceRef),
    Record(ObjRef),
    Buffer(varn_types::VmBuffer),

    Module(Rc<ModuleObj>),

    FrozenModule(Arc<FrozenModuleObj>),
    VmClosure(Rc<VmClosure>),
    Class(Rc<ClassObj>),
    NativeFn(NativeFn, &'static str),
    BoundMethod(Box<BoundMethod>),
    Map(MapRef),
    Set(SetRef),
    Task(Rc<LazyTask>),
    TaskHandle(AsyncTask),
    Range(RangeData),
    Symbol(RuntimeSymbol),
    EnumVariant(Box<EnumVariantData>),
    BigInt(Box<num_bigint::BigInt>),
    Decimal(Box<bigdecimal::BigDecimal>),
    Char(char),
    Generator(GeneratorObj),
    Spread(VmValue),
    VmValue(Box<dyn VmValuePayload>),
}

impl HeapObj {
    /// The single canonical [`RuntimeKind`] of this heap object. Callables
    /// (closure / native fn / bound method) coalesce to `Function`; modules
    /// present as `Object`; spreads as `Array`; opaque host payloads as `Opaque`.
    /// All value-kind name rendering flows through this — see [`RuntimeKind::name`].
    pub(crate) fn tag(&self) -> varn_core::RuntimeKind {
        use varn_core::RuntimeKind;
        match self {
            HeapObj::Str(_) => RuntimeKind::Str,
            HeapObj::Array(_) | HeapObj::Tuple(_) => RuntimeKind::Array,
            HeapObj::Object(_)
            | HeapObj::Instance(_)
            | HeapObj::Record(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_) => RuntimeKind::Object,
            HeapObj::VmClosure(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) => {
                RuntimeKind::Function
            }
            HeapObj::Class(_) => RuntimeKind::Class,
            HeapObj::Map(_) => RuntimeKind::Map,
            HeapObj::Set(_) => RuntimeKind::Set,
            HeapObj::Task(_) => RuntimeKind::Task,
            HeapObj::TaskHandle(_) => RuntimeKind::TaskHandle,
            HeapObj::Range(_) => RuntimeKind::Range,
            HeapObj::Symbol(_) => RuntimeKind::Symbol,
            HeapObj::EnumVariant(_) => RuntimeKind::Enum,
            HeapObj::BigInt(_) => RuntimeKind::BigInt,
            HeapObj::Decimal(_) => RuntimeKind::Decimal,
            HeapObj::Char(_) => RuntimeKind::Char,
            HeapObj::Generator(_) => RuntimeKind::Generator,
            HeapObj::Spread(_) => RuntimeKind::Array,
            HeapObj::Buffer(_) => RuntimeKind::Bytes,
            HeapObj::VmValue(_) => RuntimeKind::Opaque,
        }
    }
}
