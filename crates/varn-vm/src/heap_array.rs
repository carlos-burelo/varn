

















use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

#[inline(always)]
pub(crate) fn array_len(heap: &Heap, arr: VmValue) -> usize {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get(arr.as_heap()) {
            return a.len();
        }
    }
    0
}

#[inline(always)]
pub(crate) fn array_get(heap: &Heap, arr: VmValue, idx: usize) -> Option<VmValue> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get(arr.as_heap()) {
            return a.get_vm(idx);
        }
    }
    None
}

#[inline(always)]
pub(crate) fn array_set(heap: &mut Heap, arr: VmValue, idx: usize, val: VmValue) {
    if arr.is_heap() {
        let raw_idx = arr.as_heap();
        if let Some(HeapObj::Array(a)) = heap.get(raw_idx) {
            if a.set_vm(idx, val) {
                heap.write_barrier(raw_idx, val);
            }
        }
    }
}

#[inline(always)]
pub(crate) fn array_push(heap: &mut Heap, arr: VmValue, val: VmValue) {
    if arr.is_heap() {
        let raw_idx = arr.as_heap();
        if let Some(HeapObj::Array(a)) = heap.get(raw_idx) {
            a.push_vm(val);
            heap.write_barrier(raw_idx, val);
        }
    }
}

#[inline(always)]
pub(crate) fn array_pop(heap: &Heap, arr: VmValue) -> Option<VmValue> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get(arr.as_heap()) {
            return a.pop_vm();
        }
    }
    None
}

#[inline(always)]
pub(crate) fn array_for_each(heap: &Heap, arr: VmValue, f: &mut dyn FnMut(VmValue, usize)) {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get(arr.as_heap()) {
            for i in 0..a.len() {
                f(a.get_vm(i).unwrap(), i);
            }
        }
    }
}
