use super::str::HeapStr;
use crate::closure::VmClosure;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::Arc;
use varn_types::{
    generator::GeneratorObj,
    value::{
        BoundMethod, EnumVariantData, FrozenModuleObj, InstanceRef, MapRef, ModuleObj, ObjRef,
        RangeData, RuntimeSymbol, SetRef,
    },
    ClassObj, NativeFn, VmArray,
};



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
    Task(Rc<crate::task::LazyTask>),
    TaskHandle(Rc<crate::task::TaskCell>),
    Range(RangeData),
    Symbol(RuntimeSymbol),
    EnumVariant(Box<EnumVariantData>),
    BigInt(Box<num_bigint::BigInt>),
    Decimal(Box<bigdecimal::BigDecimal>),
    Char(char),
    Generator(GeneratorObj),
    Spread(VmValue),
}

impl HeapObj {
    
    
    
    
    pub(crate) fn tag(&self) -> varn_core::RuntimeKind {
        use varn_core::RuntimeKind;
        match self {
            HeapObj::Str(_) => RuntimeKind::Str,
            HeapObj::Array(_) => RuntimeKind::Array,
            HeapObj::Tuple(_) => RuntimeKind::Tuple,
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
        }
    }
}
