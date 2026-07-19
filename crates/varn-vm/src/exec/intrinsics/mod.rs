mod collections;
mod int;
mod math;
mod str;

use crate::error::VmResult;
use crate::heap::Heap;
use crate::value::VmValue;
use varn_core::intrinsic_ops::wire::{decode, IntrinsicDomain};

pub fn dispatch(wire_byte: u8, args: &[VmValue], heap: &mut Heap) -> VmResult<VmValue> {
    let (domain, op) = decode(wire_byte);
    match domain {
        d if d == IntrinsicDomain::Math as u8 => math::dispatch(op, args),
        d if d == IntrinsicDomain::Str as u8 => self::str::dispatch(op, args, heap),
        d if d == IntrinsicDomain::Int as u8 => self::int::dispatch(op, args, heap),
        d if d == IntrinsicDomain::Map as u8 => collections::dispatch_map(op, args, heap),
        d if d == IntrinsicDomain::Set as u8 => collections::dispatch_set(op, args, heap),
        _ => Ok(VmValue::null()),
    }
}
