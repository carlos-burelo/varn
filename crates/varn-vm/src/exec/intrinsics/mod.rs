mod math;

use crate::error::VmResult;
use crate::heap::Heap;
use crate::value::VmValue;
use varn_core::intrinsic_ops::wire::{decode, IntrinsicDomain};

pub fn dispatch(wire_byte: u8, args: &[VmValue], _heap: &mut Heap) -> VmResult<VmValue> {
    let (domain, op) = decode(wire_byte);
    match domain {
        d if d == IntrinsicDomain::Math as u8 => math::dispatch(op, args),
        _ => Ok(VmValue::null()),
    }
}
