use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::RuntimeKind;
use varn_types::{ClassObj, NativeCtx};

pub(crate) fn typeof_val(val: VmValue, heap: &Heap) -> &'static str {
    if val.is_null() {
        return RuntimeKind::Null.name();
    }
    if val.is_bool() {
        return RuntimeKind::Bool.name();
    }
    if val.is_int() {
        return RuntimeKind::Int.name();
    }
    if val.is_f64() {
        return RuntimeKind::Float.name();
    }
    if val.is_sso() {
        return RuntimeKind::Str.name();
    }
    if val.is_heap() {
        let r = val.as_heap();
        return match heap.get(r) {
            Some(obj) => obj.tag().name(),
            None if heap.instance(r).is_some() => RuntimeKind::Object.name(),
            None => varn_core::UNKNOWN,
        };
    }
    varn_core::UNKNOWN
}

pub(crate) fn instanceof(obj: VmValue, class_nv: VmValue, heap: &Heap) -> bool {
    if !class_nv.is_heap() {
        return false;
    }
    let cls = match heap.get(class_nv.as_heap()) {
        Some(HeapObj::Class(c)) => c.clone(),
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
        | None => return false,
    };

    match cls.name.as_str() {
        n if n == varn_core::RuntimeKind::Str.name() => {
            return obj.is_sso()
                || (obj.is_heap() && matches!(heap.get(obj.as_heap()), Some(HeapObj::Str(_))))
        }
        n if n == varn_core::RuntimeKind::Int.name() => {
            return obj.is_int()
                || (obj.is_f64() && {
                    let f = obj.as_f64();
                    f == f.floor()
                })
        }
        n if n == varn_core::RuntimeKind::Float.name() => return obj.is_f64() || obj.is_int(),
        n if n == varn_core::RuntimeKind::Bool.name() => return obj.is_bool(),
        n if n == varn_core::RuntimeKind::Null.name() => return obj.is_null(),
        n if n == varn_core::RuntimeKind::Char.name() => {
            return obj.is_heap() && matches!(heap.get(obj.as_heap()), Some(HeapObj::Char(_)))
        }
        n if n == varn_core::RuntimeKind::Decimal.name() => {
            return obj.is_heap() && matches!(heap.get(obj.as_heap()), Some(HeapObj::Decimal(_)))
        }
        _ => {}
    }
    if !obj.is_heap() {
        return false;
    }
    let obj_class = match heap.instance(obj.as_heap()) {
        Some(inst) => ClassObj::find_by_id(inst.class_id),
        None => match heap.get(obj.as_heap()) {
            Some(HeapObj::Object(o) | HeapObj::Record(o)) => o.borrow().class().clone(),
            Some(
                HeapObj::Str(_)
                | HeapObj::Array(_)
                | HeapObj::Tuple(_)
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
            | None => return false,
        },
    };
    let mut cur = obj_class;
    while let Some(c) = cur {
        if c.id == cls.id {
            return true;
        }
        cur = c.superclass.borrow().clone();
    }
    false
}

pub(crate) fn op_in(key: VmValue, obj: VmValue, heap: &Heap) -> bool {
    if !obj.is_heap() {
        return false;
    }
    let key_s = heap.str_repr(key);
    match heap.get(obj.as_heap()) {
        Some(HeapObj::Object(o)) => o.borrow().contains_key(&key_s),
        Some(HeapObj::Array(a)) => {
            if let Ok(idx) = key_s.parse::<usize>() {
                return idx < a.len();
            }
            false
        }
        Some(
            HeapObj::Str(_)
            | HeapObj::Tuple(_)
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
        | None => false,
    }
}

pub(crate) fn is_array(val: VmValue, heap: &Heap) -> bool {
    val.is_heap() && matches!(heap.get(val.as_heap()), Some(HeapObj::Array(_)))
}

pub(crate) fn assert_not_null(val: VmValue) -> VmResult<()> {
    if val.is_null() {
        Err(RuntimeError::new("null assertion failed"))
    } else {
        Ok(())
    }
}

pub(crate) fn get_enum_tag(val: VmValue, heap: &Heap) -> VmResult<VmValue> {
    if val.is_heap() {
        if let Some(HeapObj::EnumVariant(e)) = heap.get(val.as_heap()) {
            return Ok(VmValue::from_i32(e.variant_tag as i32));
        }
    }
    Err(RuntimeError::new("OpGetEnumTag: not an enum variant"))
}
