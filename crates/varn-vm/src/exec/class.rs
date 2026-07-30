use crate::error::{RuntimeError, VmResult};
use crate::exec::props::bind_method_to_receiver;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use std::rc::Rc;
use varn_types::{value::ObjRef, ClassObj};

pub fn op_class(name: &str, heap: &mut Heap) -> VmValue {
    let cls = ClassObj::new_rc(name);
    heap.set_intrinsic_class(name, cls.clone());
    VmValue::from_heap_idx(heap.alloc(HeapObj::Class(cls)))
}

pub fn op_method(
    class_nv: VmValue,
    name: &str,
    method_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let method_val = heap.extract(method_nv);
    let cls = get_class_arc(class_nv, heap)?;
    cls.add_method_with_owner(name, method_val, Some(cls.clone()));
    Ok(())
}

pub fn op_define_static(
    class_nv: VmValue,
    name: &str,
    val_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let val = heap.extract(val_nv);
    let cls = get_class_arc(class_nv, heap)?;
    cls.add_static(name, val);
    Ok(())
}

pub fn op_inherit(subclass_nv: VmValue, superclass_nv: VmValue, heap: &mut Heap) -> VmResult<()> {
    let superclass = get_class_arc(superclass_nv, heap)?;
    if subclass_nv.is_heap() {
        if let Some(HeapObj::Class(sub)) = heap.get_mut(subclass_nv.as_heap_idx()) {
            *sub.superclass.borrow_mut() = Some(superclass.clone());

            *sub.vtable.borrow_mut() = superclass.vtable.borrow().clone();
            *sub.vtable_owners.borrow_mut() = superclass.vtable_owners.borrow().clone();
            *sub.method_map.borrow_mut() = superclass.method_map.borrow().clone();

            let mut existing_sub_fields: Vec<(varn_types::RuntimeString, usize)> = sub
                .root_shape
                .borrow()
                .property_names
                .iter()
                .map(|(k, &v)| (k.clone(), v))
                .collect();
            existing_sub_fields.sort_by_key(|(_, slot)| *slot);

            let mut new_root = superclass.root_shape.borrow().with_class(Some(sub.clone()));
            for (name, _) in existing_sub_fields {
                if !new_root.property_names.contains_key(&name) {
                    new_root = new_root.transition(name);
                }
            }
            *sub.root_shape.borrow_mut() = new_root;

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

pub fn op_declare_field(class_nv: VmValue, name: &str, heap: &mut Heap) -> VmResult<()> {
    if class_nv.is_heap() {
        if let Some(HeapObj::Class(cls)) = heap.get_mut(class_nv.as_heap_idx()) {
            cls.declare_field(Rc::from(name));
            return Ok(());
        }
    }
    let got = heap.extract(class_nv);
    Err(RuntimeError::new(format!(
        "OpDeclareField: expected class, got {}",
        got.type_name()
    )))
}

pub fn op_define_getter(
    class_nv: VmValue,
    name: &str,
    closure_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    let val = heap.extract(closure_nv);
    cls.add_getter_with_owner(name, val, Some(cls.clone()));
    Ok(())
}

pub fn op_define_setter(
    class_nv: VmValue,
    name: &str,
    closure_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    let val = heap.extract(closure_nv);
    cls.add_setter_with_owner(name, val, Some(cls.clone()));
    Ok(())
}

pub fn op_define_static_getter(
    class_nv: VmValue,
    name: &str,
    closure_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    let val = heap.extract(closure_nv);
    cls.add_static_getter(name, val);
    Ok(())
}

pub fn op_define_static_setter(
    class_nv: VmValue,
    name: &str,
    closure_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<()> {
    let cls = get_class_arc(class_nv, heap)?;
    let val = heap.extract(closure_nv);
    cls.add_static_setter(name, val);
    Ok(())
}

pub fn op_get_super(
    class_nv: VmValue,
    name: &str,
    receiver_nv: VmValue,
    heap: &mut Heap,
) -> VmResult<VmValue> {
    let cls = get_class_arc(class_nv, heap)?;
    let super_cls = cls
        .superclass
        .borrow()
        .as_ref()
        .ok_or_else(|| RuntimeError::new("no superclass"))?
        .clone();
    let method = super_cls
        .find_method(name)
        .ok_or_else(|| RuntimeError::new(format!("super: method '{}' not found", name)))?;
    let receiver = heap.extract(receiver_nv);
    let bound = bind_method_to_receiver(receiver, method, Some(super_cls.clone()));
    Ok(heap.intern(bound))
}

pub fn new_instance(class_nv: VmValue, heap: &mut Heap) -> VmResult<VmValue> {
    let cls = get_class_arc(class_nv, heap)?;
    let oref = ObjRef::instance(cls);
    Ok(VmValue::from_heap_idx(heap.alloc(HeapObj::Object(oref))))
}

fn get_class_arc(nv: VmValue, heap: &Heap) -> VmResult<Rc<ClassObj>> {
    if nv.is_heap() {
        if let Some(HeapObj::Class(c)) = heap.get(nv.as_heap_idx()) {
            return Ok(c.clone());
        }
    }
    Err(RuntimeError::new("expected class"))
}
