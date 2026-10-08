use std::rc::Rc;

use varn_core::MemberKey;
use varn_types::value::{find_method_with_owner, ClassObj};

use crate::generator::generator_next;
use crate::heap::{Heap, HeapInner, HeapObj};
use crate::value::VmValue;

pub(super) fn class_for_property(val: VmValue, heap: &Heap) -> Option<Rc<ClassObj>> {
    if let Some(cls) = get_class(val, heap) {
        return Some(cls);
    }
    if !val.is_heap() {
        return None;
    }
    let kind = match heap.get(val.as_heap())? {
        HeapObj::Symbol(_) => varn_core::RuntimeKind::Symbol,
        HeapObj::BigInt(_) => varn_core::RuntimeKind::BigInt,
        HeapObj::Decimal(_) => varn_core::RuntimeKind::Decimal,
        HeapObj::Char(_) => varn_core::RuntimeKind::Char,
        HeapObj::Str(_)
        | HeapObj::Array(_)
        | HeapObj::Tuple(_)
        | HeapObj::Object(_)
        | HeapObj::Record(_)
        | HeapObj::Buffer(_)
        | HeapObj::Module(_)
        | HeapObj::FrozenModule(_)
        | HeapObj::VmClosure(_)
        | HeapObj::Class(_)
        | HeapObj::NativeFn(..)
        | HeapObj::BoundMethod(_)
        | HeapObj::Map(_)
        | HeapObj::Set(_)
        | HeapObj::Task(_)
        | HeapObj::TaskHandle(_)
        | HeapObj::Range(_)
        | HeapObj::EnumVariant(_)
        | HeapObj::Generator(_)
        | HeapObj::Spread(_) => return None,
    };
    heap.get_intrinsic_class(kind.name())
}

pub(super) fn resolve_intrinsic_method_property(
    obj: VmValue,
    key: &str,
    heap: &mut Heap,
) -> Option<VmValue> {
    if obj.is_heap() {
        match heap.get(obj.as_heap()) {
            Some(HeapObj::Generator(_)) if key == MemberKey::IterNext.as_str() => {
                return Some(heap.alloc_bound_native(
                    obj,
                    generator_next,
                    MemberKey::IterNext.as_str(),
                ));
            }
            Some(HeapObj::EnumVariant(_)) if key == MemberKey::Name.as_str() => return None,
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Tuple(_)
                | HeapObj::Object(_)
                | HeapObj::Record(_)
                | HeapObj::Buffer(_)
                | HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::Class(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Map(_)
                | HeapObj::Set(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Range(_)
                | HeapObj::Symbol(_)
                | HeapObj::EnumVariant(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Generator(_)
                | HeapObj::Spread(_),
            )
            | None => {}
        }
    }
    let cls = class_for_property(obj, heap)?;
    if let Some((method, owner)) = find_method_with_owner(&cls, key) {
        return Some(bind_method_to_receiver(heap, obj, method, Some(owner)));
    }
    if key == MemberKey::Name.as_str() {
        return Some(HeapInner::alloc_str(heap, cls.name.as_str()));
    }
    None
}

pub(crate) fn get_class(val: VmValue, heap: &Heap) -> Option<Rc<ClassObj>> {
    if val.is_heap() {
        let intrinsic = |kind: varn_core::RuntimeKind| heap.get_intrinsic_class(kind.name());
        if let Some(inst) = heap.instance_of(val) {
            return ClassObj::find_by_id(inst.class_id);
        }
        return match heap.get(val.as_heap()) {
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => o.borrow().class(),
            Some(HeapObj::Class(cls)) => Some(cls.clone()),
            Some(HeapObj::Array(_) | HeapObj::Tuple(_)) => intrinsic(varn_core::RuntimeKind::Array),
            Some(HeapObj::Str(_)) => intrinsic(varn_core::RuntimeKind::Str),
            Some(HeapObj::Map(_)) => intrinsic(varn_core::RuntimeKind::Map),
            Some(HeapObj::Set(_)) => intrinsic(varn_core::RuntimeKind::Set),
            Some(HeapObj::EnumVariant(ev)) => ev.enum_class_id.and_then(ClassObj::find_by_id),
            Some(HeapObj::Range(_)) => intrinsic(varn_core::RuntimeKind::Range),
            Some(HeapObj::Buffer(_)) => intrinsic(varn_core::RuntimeKind::Bytes),
            Some(HeapObj::Generator(_)) => intrinsic(varn_core::RuntimeKind::Generator),
            Some(
                HeapObj::Module(_)
                | HeapObj::FrozenModule(_)
                | HeapObj::VmClosure(_)
                | HeapObj::NativeFn(..)
                | HeapObj::BoundMethod(_)
                | HeapObj::Task(_)
                | HeapObj::TaskHandle(_)
                | HeapObj::Symbol(_)
                | HeapObj::BigInt(_)
                | HeapObj::Decimal(_)
                | HeapObj::Char(_)
                | HeapObj::Spread(_),
            )
            | None => None,
        };
    }
    let kind = if val.is_int() {
        varn_core::RuntimeKind::Int
    } else if val.is_f64() {
        varn_core::RuntimeKind::Float
    } else if val.is_bool() {
        varn_core::RuntimeKind::Bool
    } else if val.is_sso() {
        varn_core::RuntimeKind::Str
    } else if val.is_null() {
        varn_core::RuntimeKind::Null
    } else {
        return None;
    };
    heap.get_intrinsic_class(kind.name())
}

pub(crate) fn bind_method_to_receiver(
    heap: &mut Heap,
    receiver: VmValue,
    method: VmValue,
    owner: Option<Rc<ClassObj>>,
) -> VmValue {
    if !method.is_heap() {
        return method;
    }
    match heap.get(method.as_heap()) {
        Some(HeapObj::VmClosure(_)) => heap.alloc_bound_vm(receiver, method, owner),
        Some(HeapObj::NativeFn(f, name)) => {
            let (f, name) = (*f, *name);
            heap.alloc_bound_native(receiver, f, name)
        }
        Some(
            HeapObj::Str(_)
            | HeapObj::Array(_)
            | HeapObj::Tuple(_)
            | HeapObj::Object(_)
            | HeapObj::Record(_)
            | HeapObj::Buffer(_)
            | HeapObj::Module(_)
            | HeapObj::FrozenModule(_)
            | HeapObj::Class(_)
            | HeapObj::BoundMethod(_)
            | HeapObj::Map(_)
            | HeapObj::Set(_)
            | HeapObj::Task(_)
            | HeapObj::TaskHandle(_)
            | HeapObj::Range(_)
            | HeapObj::Symbol(_)
            | HeapObj::EnumVariant(_)
            | HeapObj::BigInt(_)
            | HeapObj::Decimal(_)
            | HeapObj::Char(_)
            | HeapObj::Generator(_)
            | HeapObj::Spread(_),
        )
        | None => method,
    }
}
