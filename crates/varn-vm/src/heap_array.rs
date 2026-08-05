//! Single shared implementation of `NativeCtx` array element access.
//!
//! `impl NativeCtx for Heap` (heap.rs) and `impl NativeCtx for ExecCtx`
//! (exec/frame_ctrl.rs) both need `array_len/get/set/push/pop/for_each`.
//! Production only ever dispatches through the `ExecCtx` impl, while the
//! `Heap` impl is what the unit tests exercise directly — two copies of the
//! same logic meant an edit to one twin could pass every test while still
//! breaking production. Both impls now delegate here so there is exactly
//! one place that knows the barrier semantics.
//!
//! Barrier semantics (preserved exactly, do not change per-caller):
//! - `array_set`: barrier only when `VmArray::set_vm` reports the slot was
//!   written (in-bounds); an out-of-bounds `set` is a silent no-op with no
//!   barrier.
//! - `array_push`: barrier unconditionally — a push always writes a slot.
//! - `array_pop`: no barrier — removing a value can't introduce a new
//!   old-gen -> nursery edge.

use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;

#[inline(always)]
pub(crate) fn array_len(heap: &Heap, arr: VmValue) -> usize {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get_by_idx(arr.as_heap_idx()) {
            return a.len();
        }
    }
    0
}

#[inline(always)]
pub(crate) fn array_get(heap: &Heap, arr: VmValue, idx: usize) -> Option<VmValue> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get_by_idx(arr.as_heap_idx()) {
            return a.get_vm(idx);
        }
    }
    None
}

#[inline(always)]
pub(crate) fn array_set(heap: &mut Heap, arr: VmValue, idx: usize, val: VmValue) {
    if arr.is_heap() {
        let raw_idx = arr.as_heap_idx();
        if let Some(HeapObj::Array(a)) = heap.get_by_idx(raw_idx) {
            if a.set_vm(idx, val) {
                heap.write_barrier(raw_idx, val);
            }
        }
    }
}

#[inline(always)]
pub(crate) fn array_push(heap: &mut Heap, arr: VmValue, val: VmValue) {
    if arr.is_heap() {
        let raw_idx = arr.as_heap_idx();
        if let Some(HeapObj::Array(a)) = heap.get_by_idx(raw_idx) {
            a.push_vm(val);
            heap.write_barrier(raw_idx, val);
        }
    }
}

#[inline(always)]
pub(crate) fn array_pop(heap: &Heap, arr: VmValue) -> Option<VmValue> {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a)) = heap.get_by_idx(arr.as_heap_idx()) {
            return a.pop_vm();
        }
    }
    None
}

#[inline(always)]
pub(crate) fn array_for_each(heap: &Heap, arr: VmValue, f: &mut dyn FnMut(VmValue, usize)) {
    if arr.is_heap() {
        if let Some(HeapObj::Array(a) | HeapObj::Tuple(a)) = heap.get_by_idx(arr.as_heap_idx()) {
            for i in 0..a.len() {
                f(a.get_vm(i).unwrap(), i);
            }
        }
    }
}
