use crate::error::VmResult;
use crate::heap::{Heap, HeapObj};
use crate::value::VmValue;
use varn_core::intrinsic_ops::array::ArrayOp;

pub fn dispatch(op: u8, args: &[VmValue], heap: &mut Heap) -> VmResult<VmValue> {
    let Some(arg0) = args.first().copied() else {
        return Ok(VmValue::null());
    };
    if !arg0.is_heap() {
        return Ok(VmValue::null());
    }
    let idx = arg0.as_heap_idx();
    match op {
        v if v == ArrayOp::Len as u8 => {
            if let Some(HeapObj::Array(arr)) = heap.get(idx) {
                return Ok(VmValue::from_int(arr.len() as i64));
            }
            Ok(VmValue::null())
        }
        v if v == ArrayOp::Push as u8 => {
            let val = args.get(1).copied().unwrap_or_else(VmValue::null);
            if let Some(HeapObj::Array(arr)) = heap.get_mut(idx) {
                arr.borrow_mut().push(val);
            }
            Ok(VmValue::null())
        }
        v if v == ArrayOp::Pop as u8 => {
            if let Some(HeapObj::Array(arr)) = heap.get_mut(idx) {
                return Ok(arr.borrow_mut().pop().unwrap_or_else(VmValue::null));
            }
            Ok(VmValue::null())
        }
        v if v == ArrayOp::Contains as u8 => {
            let needle = args.get(1).copied().unwrap_or_else(VmValue::null);
            if let Some(HeapObj::Array(arr)) = heap.get(idx) {
                let found = arr.borrow().iter().any(|&v| v == needle);
                return Ok(VmValue::from_bool(found));
            }
            Ok(VmValue::from_bool(false))
        }
        _ => Ok(VmValue::null()),
    }
}
