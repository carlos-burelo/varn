mod math;

use crate::error::VmResult;
use crate::value::VmValue;

/// `OpCode::IntrinsicDirect` — a unary math op on one value, with no
/// receiver slot. Routed through the very same [`math::dispatch`] as the
/// windowed form (fed a synthetic receiver) so the two encodings cannot
/// drift apart in the int-result re-boxing or anywhere else.
pub(crate) fn dispatch_unary(wire_byte: u8, x: VmValue) -> VmResult<VmValue> {
    math::dispatch(wire_byte, &[VmValue::null(), x])
}

/// `OpCode::Intrinsic` — a `std:math` op (`varn_core::intrinsic_ops`).
pub(crate) fn dispatch(wire_byte: u8, args: &[VmValue]) -> VmResult<VmValue> {
    math::dispatch(wire_byte, args)
}
