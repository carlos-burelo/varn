use std::rc::Rc;

use varn_types::value::{ClassObj, InstanceRef, ObjRef};

use super::payload_object;
use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

#[inline(always)]
pub(crate) fn get_fixed_field_at(
    obj: VmValue,
    offset: u32,
    tag: Option<varn_core::RuntimeKind>,
    heap: &Heap,
) -> VmResult<VmValue> {
    if obj.is_heap() {
        if let Some(HeapObj::Instance(inst)) = heap.get(obj.as_heap_idx()) {
            if let Some(v) = inst.read_field_at(offset, tag) {
                return Ok(v);
            }
        }
    }
    Err(RuntimeError::new(format!(
        "OpGetFixedField: bad compact field offset {offset} on {obj:?}"
    )))
}

pub(crate) fn set_fixed_field_at(
    obj: VmValue,
    offset: u32,
    tag: Option<varn_core::RuntimeKind>,
    val: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    if obj.is_heap() {
        if let Some(HeapObj::Instance(inst)) = heap.get(obj.as_heap_idx()) {
            if inst.write_field_at(offset, tag, val).is_ok() {
                heap.write_barrier(obj.as_heap_idx(), val);
                return Ok(());
            }
        }
    }
    Err(RuntimeError::new(format!(
        "OpSetFixedField: bad compact field offset {offset} on {obj:?}"
    )))
}

pub(crate) fn get_fixed_field(obj: VmValue, slot: usize, heap: &mut Heap) -> VmResult<VmValue> {
    if obj.is_heap() {
        let found = match heap.get(obj.as_heap_idx()) {
            Some(HeapObj::Instance(inst)) => inst.field_at(slot),
            Some(HeapObj::Object(o)) | Some(HeapObj::Record(o)) => o.field_at(slot),
            Some(HeapObj::EnumVariant(ev)) => {
                payload_object(heap, ev.payload).and_then(|o| o.field_at(slot))
            }
            Some(HeapObj::Class(cls)) => {
                let fields = cls.static_fields.borrow();
                fields
                    .get(slot)
                    .and_then(|name| cls.statics.borrow().get(name).copied())
            }
            _ => None,
        };
        if let Some(v) = found {
            return Ok(v);
        }
    }
    let details = if obj.is_heap() {
        match heap.get(obj.as_heap_idx()) {
            Some(HeapObj::Object(o)) => format!(
                "Object[inline_len={}, slot_count={}, props={:?}]",
                o.inline_len(),
                o.slot_count(),
                o.shape().property_names
            ),
            Some(other) => format!("{:?}", other),
            None => "None".to_string(),
        }
    } else {
        format!("{:?}", obj)
    };
    Err(RuntimeError::new(format!(
        "OpGetFixedField: slot {} out of range on obj {:?} (details: {})",
        slot, obj, details
    )))
}

#[inline(always)]
pub(crate) fn set_fixed_field(
    obj: VmValue,
    slot: usize,
    val: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();

        enum Target {
            Instance(InstanceRef),
            Obj(ObjRef, u32),
            Class(Rc<ClassObj>),
        }

        let target = match heap.get(heap_idx) {
            Some(HeapObj::Instance(inst)) => Some(Target::Instance(inst.clone())),
            Some(HeapObj::Object(o)) => Some(Target::Obj(o.clone(), heap_idx)),
            Some(HeapObj::EnumVariant(ev)) => {
                payload_object(heap, ev.payload).map(|o| Target::Obj(o, ev.payload.as_heap_idx()))
            }
            Some(HeapObj::Class(cls)) => Some(Target::Class(cls.clone())),
            _ => None,
        };

        match target {
            Some(Target::Instance(inst)) => {
                if inst.set_field_at(slot, val) {
                    heap.write_barrier(heap_idx, val);
                    return Ok(());
                }
            }
            Some(Target::Obj(o, owner_idx)) => {
                if o.set_field_at(slot, val) {
                    heap.write_barrier(owner_idx, val);
                    return Ok(());
                }
            }
            Some(Target::Class(cls)) => {
                let name = cls.static_fields.borrow().get(slot).cloned();
                if let Some(name) = name {
                    cls.statics.borrow_mut().insert(name, val);
                    return Ok(());
                }
            }
            None => {}
        }
    }
    Err(RuntimeError::new(
        "OpSetFixedField: slot out of range or invalid target",
    ))
}
