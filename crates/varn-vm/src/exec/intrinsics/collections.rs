//! `Map`/`Set` hot-path intrinsics: the receiver's backing table is reached
//! directly on the heap — no argument marshalling, no contract wrapper, no
//! fat-value round trip. Key canonicalization matches the native builtins
//! (`Heap::canonical_map_key` on insert, `Heap::lookup_map_key` on lookup,
//! where `None` means the key cannot be present).

use crate::error::{RuntimeError, VmResult};
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::intrinsic_ops::collections::{MapOp, SetOp};
use varn_types::value::{MapRef, SetRef};

fn map_of(heap: &Heap, v: VmValue) -> VmResult<MapRef> {
    if v.is_heap() {
        if let Some(HeapObj::Map(m)) = heap.get(v.as_heap_idx()) {
            return Ok(m.clone());
        }
    }
    Err(RuntimeError::new("Map intrinsic: receiver is not a Map"))
}

fn set_of(heap: &Heap, v: VmValue) -> VmResult<SetRef> {
    if v.is_heap() {
        if let Some(HeapObj::Set(s)) = heap.get(v.as_heap_idx()) {
            return Ok(s.clone());
        }
    }
    Err(RuntimeError::new("Set intrinsic: receiver is not a Set"))
}

fn arg(args: &[VmValue], i: usize) -> VmValue {
    args.get(i).copied().unwrap_or_else(VmValue::null)
}

pub(crate) fn dispatch_map(op: u8, args: &[VmValue], heap: &mut Heap) -> VmResult<VmValue> {
    let recv = arg(args, 0);
    let m = map_of(heap, recv)?;
    match op {
        o if o == MapOp::Get as u8 => {
            let found = heap
                .lookup_map_key(arg(args, 1))
                .and_then(|k| m.borrow().get(&k).copied());
            Ok(found.unwrap_or_else(VmValue::null))
        }
        o if o == MapOp::Set as u8 => {
            let k = heap.canonical_map_key(arg(args, 1));
            let value = arg(args, 2);
            m.borrow_mut().insert(k, value);
            // Interior-mutability store: no opcode barrier sees it.
            heap.write_barrier(recv.as_heap_idx(), value);
            Ok(VmValue::null())
        }
        o if o == MapOp::Has as u8 => {
            let has = heap
                .lookup_map_key(arg(args, 1))
                .map(|k| m.borrow().contains_key(&k))
                .unwrap_or(false);
            Ok(VmValue::from_bool(has))
        }
        o if o == MapOp::Delete as u8 => {
            let removed = heap
                .lookup_map_key(arg(args, 1))
                .map(|k| m.borrow_mut().remove(&k).is_some())
                .unwrap_or(false);
            Ok(VmValue::from_bool(removed))
        }
        o if o == MapOp::Clear as u8 => {
            m.borrow_mut().clear();
            Ok(VmValue::null())
        }
        _ => Err(RuntimeError::new("Map intrinsic: unknown op")),
    }
}

pub(crate) fn dispatch_set(op: u8, args: &[VmValue], heap: &mut Heap) -> VmResult<VmValue> {
    let recv = arg(args, 0);
    let s = set_of(heap, recv)?;
    match op {
        o if o == SetOp::Add as u8 => {
            let k = heap.canonical_map_key(arg(args, 1));
            s.borrow_mut().insert(k);
            // Identity keys can hold nursery indices (see the builtin).
            heap.write_barrier(recv.as_heap_idx(), k.0);
            Ok(VmValue::null())
        }
        o if o == SetOp::Has as u8 => {
            let has = heap
                .lookup_map_key(arg(args, 1))
                .map(|k| s.borrow().contains(&k))
                .unwrap_or(false);
            Ok(VmValue::from_bool(has))
        }
        o if o == SetOp::Delete as u8 => {
            let removed = heap
                .lookup_map_key(arg(args, 1))
                .map(|k| s.borrow_mut().remove(&k))
                .unwrap_or(false);
            Ok(VmValue::from_bool(removed))
        }
        o if o == SetOp::Clear as u8 => {
            s.borrow_mut().clear();
            Ok(VmValue::null())
        }
        _ => Err(RuntimeError::new("Set intrinsic: unknown op")),
    }
}
