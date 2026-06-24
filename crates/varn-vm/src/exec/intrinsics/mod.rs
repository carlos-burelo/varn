mod array;
mod math;
mod string;
mod types;

use crate::error::VmResult;
use crate::heap::Heap;
use crate::value::VmValue;
use varn_core::intrinsic_ops::wire::{decode, IntrinsicDomain};

pub fn dispatch(wire_byte: u8, args: &[VmValue], heap: &mut Heap) -> VmResult<VmValue> {
    let (domain, op) = decode(wire_byte);
    match domain {
        d if d == IntrinsicDomain::Math as u8 => math::dispatch(op, args),
        d if d == IntrinsicDomain::String as u8 => string::dispatch(op, args, heap),
        d if d == IntrinsicDomain::Array as u8 => array::dispatch(op, args, heap),
        d if d == IntrinsicDomain::TypeCheck as u8 => types::dispatch(op, args),
        _ => Ok(VmValue::null()),
    }
}
