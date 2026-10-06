mod math;

use crate::error::VmResult;
use crate::value::VmValue;





pub(crate) fn dispatch_unary(wire_byte: u8, x: VmValue) -> VmResult<VmValue> {
    math::dispatch(wire_byte, &[VmValue::null(), x])
}


pub(crate) fn dispatch(wire_byte: u8, args: &[VmValue]) -> VmResult<VmValue> {
    math::dispatch(wire_byte, args)
}
