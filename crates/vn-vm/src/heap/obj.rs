use super::str::HeapStr;
use crate::closure::VmClosure;
use crate::value::VmValue;
use std::rc::Rc;
use std::sync::Arc;
use vn_types::{
    value::{
        BoundMethod, EnumVariantData, FrozenModuleObj, MapRef, ModuleObj, ObjRef, RangeData, SetRef,
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
    Buffer(vn_types::VmBuffer),

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
    EnumVariant(Box<EnumVariantData>),
    BigInt(Box<num_bigint::BigInt>),
    Decimal(Box<bigdecimal::BigDecimal>),
    Char(char),
    Generator(Rc<crate::generator::GenState>),
    Spread(VmValue),
}

pub(crate) enum DeadEntry {
    Identity(usize),
    Interned(crate::heap::cells::SweptInterned),
}

impl HeapObj {
    pub(crate) fn dead_entry(&self) -> Option<DeadEntry> {
        match self {
            HeapObj::Class(c) => Some(DeadEntry::Identity(Rc::as_ptr(c) as usize)),
            HeapObj::Generator(g) => Some(DeadEntry::Identity(Rc::as_ptr(g) as usize)),
            HeapObj::VmClosure(c) => Some(DeadEntry::Identity(Rc::as_ptr(c) as usize)),
            HeapObj::Str(s) if s.is_interned() => Some(DeadEntry::Interned(
                crate::heap::cells::SweptInterned::Str(s.to_shared()),
            )),
            HeapObj::Char(c) => Some(DeadEntry::Interned(
                crate::heap::cells::SweptInterned::Char(*c),
            )),
            HeapObj::BigInt(n) => Some(DeadEntry::Interned(
                crate::heap::cells::SweptInterned::BigInt(n.clone()),
            )),
            HeapObj::Decimal(d) => Some(DeadEntry::Interned(
                crate::heap::cells::SweptInterned::Decimal(d.clone()),
            )),
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Object(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::NativeFn(..)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::Spread(_) => None,
        }
    }

    pub(crate) fn header_bits(&self) -> u8 {
        use vn_types::cell::{STATE_NEEDS_DROP, STATE_SIDE_TABLE};
        match self {
            HeapObj::Class(_) | HeapObj::Generator(_) | HeapObj::VmClosure(_) | HeapObj::Str(_) => {
                STATE_NEEDS_DROP | STATE_SIDE_TABLE
            }
            HeapObj::Char(_) => STATE_SIDE_TABLE,
            HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Object(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::EnumVariant(_) => STATE_NEEDS_DROP,
            HeapObj::NativeFn(..) | HeapObj::Spread(_) => 0,
        }
    }

    pub(crate) fn tag(&self) -> vn_core::RuntimeKind {
        use vn_core::RuntimeKind;
        match self {
            HeapObj::Str(_) => RuntimeKind::Str,
            HeapObj::Array(_) => RuntimeKind::Array,
            HeapObj::Tuple(_) => RuntimeKind::Tuple,
            HeapObj::Object(_) | HeapObj::Module(_) | HeapObj::FrozenModule(_) => {
                RuntimeKind::Object
            }
            HeapObj::VmClosure(_) | HeapObj::NativeFn(..) | HeapObj::BoundMethod(_) => {
                RuntimeKind::Function
            }
            HeapObj::Class(_) => RuntimeKind::Class,
            HeapObj::Map(_) => RuntimeKind::Map,
            HeapObj::Set(_) => RuntimeKind::Set,
            HeapObj::Task(_) => RuntimeKind::Task,
            HeapObj::TaskHandle(_) => RuntimeKind::TaskHandle,
            HeapObj::Range(_) => RuntimeKind::Range,
            HeapObj::EnumVariant(_) => RuntimeKind::Enum,
            HeapObj::BigInt(_) => RuntimeKind::BigInt,
            HeapObj::Decimal(_) => RuntimeKind::Decimal,
            HeapObj::Char(_) => RuntimeKind::Char,
            HeapObj::Generator(g) if g.is_async => RuntimeKind::AsyncGenerator,
            HeapObj::Generator(_) => RuntimeKind::Generator,
            HeapObj::Spread(_) => RuntimeKind::Array,
            HeapObj::Buffer(_) => RuntimeKind::Bytes,
        }
    }
}
