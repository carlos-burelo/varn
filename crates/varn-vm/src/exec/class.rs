use crate::error::{RuntimeError, VmResult};
use crate::exec::props::bind_method_to_receiver;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::ClassObj;

pub(crate) fn op_class(name: &str, heap: &mut Heap) -> VmValue {
    let cls = ClassObj::new_rc(name);
    VmValue::from_heap(heap.alloc(HeapObj::Class(cls)))
}

pub(crate) fn op_method(
    class_nv: VmValue,
    name: &str,
    method_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    cls.add_method_with_owner(name, method_nv, Some(cls.clone()));
    Ok(())
}

pub(crate) fn op_define_static(
    class_nv: VmValue,
    name: &str,
    val_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;

    if val_nv.is_heap() {
        if let Some(HeapObj::EnumVariant(ev)) = heap.get_mut(val_nv.as_heap()) {
            ev.enum_class_id = Some(cls.id);
        }
    }
    cls.add_static(name, val_nv);
    Ok(())
}

pub(crate) fn op_inherit(
    subclass_nv: VmValue,
    superclass_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let superclass = get_class_arc(superclass_nv, heap)?;
    if subclass_nv.is_heap() {
        if let Some(HeapObj::Class(sub)) = heap.get_mut(subclass_nv.as_heap()) {
            *sub.superclass.borrow_mut() = Some(superclass.clone());

            *sub.vtable.borrow_mut() = superclass.vtable.borrow().clone();
            *sub.vtable_owners.borrow_mut() = superclass.vtable_owners.borrow().clone();
            *sub.method_map.borrow_mut() = superclass.method_map.borrow().clone();

            sub.set_layout(superclass.layout());

            *sub.getter_map.borrow_mut() = superclass.getter_map.borrow().clone();
            *sub.getter_vtable.borrow_mut() = superclass.getter_vtable.borrow().clone();
            *sub.getter_vtable_owners.borrow_mut() =
                superclass.getter_vtable_owners.borrow().clone();

            *sub.setter_map.borrow_mut() = superclass.setter_map.borrow().clone();
            *sub.setter_vtable.borrow_mut() = superclass.setter_vtable.borrow().clone();
            *sub.setter_vtable_owners.borrow_mut() =
                superclass.setter_vtable_owners.borrow().clone();

            return Ok(());
        }
    }
    Err(RuntimeError::new("OpInherit: not a class"))
}

pub(crate) fn op_declare_layout(
    class_nv: VmValue,
    layout: Rc<varn_core::layout::ClassLayout>,
    heap: &mut Heap,
) -> VmResult<()> {
    if class_nv.is_heap() {
        if let Some(HeapObj::Class(cls)) = heap.get_mut(class_nv.as_heap()) {
            cls.set_layout(layout);
            return Ok(());
        }
    }
    Err(RuntimeError::new(format!(
        "OpDeclareLayout: expected class, got {}",
        crate::exec::props::meta::type_name(class_nv, heap)
    )))
}

pub(crate) fn op_alloc_instance(class_nv: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    if class_nv.is_heap() {
        if let Some(HeapObj::Class(cls)) = heap.get(class_nv.as_heap()) {
            let cls = cls.clone();
            return Ok(VmValue::from_heap(heap.alloc_instance(&cls).0));
        }
    }
    Err(RuntimeError::new(format!(
        "new: expected a class, got {}",
        crate::exec::props::meta::type_name(class_nv, heap)
    )))
}

pub(crate) fn pool_layout(
    proto: &varn_types::FunctionProto,
    idx: usize,
) -> VmResult<Rc<varn_core::layout::ClassLayout>> {
    match proto.chunk.constants.get(idx) {
        Some(varn_types::PoolEntry::Layout(layout)) => Ok(Rc::clone(layout)),
        Some(varn_types::PoolEntry::Literal(_) | varn_types::PoolEntry::Function(_) | varn_types::PoolEntry::Shape(_)) | None => Err(RuntimeError::new("DeclareLayout: not a layout constant")),
    }
}

pub(crate) fn op_define_getter(
    class_nv: VmValue,
    name: &str,
    closure_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    cls.add_getter_with_owner(name, closure_nv, Some(cls.clone()));
    Ok(())
}

pub(crate) fn op_define_setter(
    class_nv: VmValue,
    name: &str,
    closure_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    cls.add_setter_with_owner(name, closure_nv, Some(cls.clone()));
    Ok(())
}

pub(crate) fn op_define_static_getter(
    class_nv: VmValue,
    name: &str,
    closure_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    cls.add_static_getter(name, closure_nv);
    Ok(())
}

pub(crate) fn op_define_static_setter(
    class_nv: VmValue,
    name: &str,
    closure_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    cls.add_static_setter(name, closure_nv);
    Ok(())
}

pub(crate) fn op_get_super(
    cls: Rc<ClassObj>,
    name: &str,
    receiver_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<VmValue> {
    let super_cls = cls
        .superclass
        .borrow()
        .as_ref()
        .ok_or_else(|| RuntimeError::new("no superclass"))?
        .clone();
    let method = super_cls
        .find_method(name)
        .ok_or_else(|| RuntimeError::new(format!("super: method '{}' not found", name)))?;
    Ok(bind_method_to_receiver(
        heap,
        receiver_nv,
        method,
        Some(super_cls.clone()),
    ))
}

fn get_class_arc(nv: VmValue, heap: &Heap) -> VmResult<Rc<ClassObj>> {
    if nv.is_heap() {
        if let Some(HeapObj::Class(c)) = heap.get(nv.as_heap()) {
            return Ok(c.clone());
        }
    }
    Err(RuntimeError::new("expected class"))
}
