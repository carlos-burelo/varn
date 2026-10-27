use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

pub(crate) fn object_entries(
    heap: &crate::heap::structs::HeapInner,
    v: VmValue,
) -> Option<Vec<(String, VmValue)>> {
    if !v.is_heap() {
        return None;
    }
    let Some(HeapObj::Object(obj) | HeapObj::Record(obj)) = heap.get(v.as_heap()) else {
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

pub(crate) fn elements(heap: &Heap, v: VmValue) -> Option<Vec<VmValue>> {
    if !v.is_heap() {
        return None;
    }
    let Some(HeapObj::Array(arr) | HeapObj::Tuple(arr)) = heap.get(v.as_heap()) else {
        return None;
    };
    Some((0..arr.len()).filter_map(|i| arr.get_vm(i)).collect())
}
