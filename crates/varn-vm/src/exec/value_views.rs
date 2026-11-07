use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

pub(crate) fn object_entries(
    heap: &crate::heap::structs::HeapInner,
    v: VmValue,
) -> Option<Vec<(String, VmValue)>> {
    if !v.is_heap() {
        return None;
    }
    let Some(HeapObj::Object(obj)) = heap.get(v.as_heap()) else {
        return None;
    };
    let obj = obj.borrow();
    let mut named: Vec<(usize, String)> = obj
        .shape()
        .property_names
        .iter()
        .map(|(name, slot)| (*slot, name.to_string()))
        .collect();
    named.sort_by_key(|(slot, _)| *slot);
    Some(
        named
            .into_iter()
            .filter_map(|(slot, name)| Some((name, obj.field_at(slot)?)))
            .collect(),
    )
}

pub(crate) struct ObjectLayout {
    pub shape: std::rc::Rc<varn_types::Shape>,
    pub entries: Vec<(std::sync::Arc<str>, VmValue)>,
}

pub(crate) fn object_layout(
    heap: &crate::heap::structs::HeapInner,
    v: VmValue,
) -> Option<ObjectLayout> {
    if !v.is_heap() {
        return None;
    }
    let Some(HeapObj::Object(obj)) = heap.get(v.as_heap()) else {
        return None;
    };
    let obj = obj.borrow();
    let shape = std::rc::Rc::clone(obj.shape());
    let entries = shape
        .ordered_names()
        .iter()
        .enumerate()
        .filter_map(|(slot, name)| Some((std::sync::Arc::clone(name), obj.field_at(slot)?)))
        .collect();
    Some(ObjectLayout { shape, entries })
}

pub(crate) fn elements(heap: &Heap, v: VmValue) -> Option<Vec<VmValue>> {
    if !v.is_heap() {
        return None;
    }
    let Some(HeapObj::Array(arr) | HeapObj::Tuple(arr)) = heap.get(v.as_heap()) else {
        return None;
    };
    Some((0..arr.len()).filter_map(|i| arr.get_vm(i)).collect())
}
