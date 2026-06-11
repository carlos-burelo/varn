use crate::error::VmResult;
use crate::value::VmValue;
use varn_core::intrinsic_ops::types::TypeCheckOp;

pub fn dispatch(op: u8, args: &[VmValue]) -> VmResult<VmValue> {
    // args[0] is the module object (dest); real arg is args[1]
    let Some(arg1) = args.get(1).copied() else {
        return Ok(VmValue::from_bool(false));
    };
    let result = match op {
        v if v == TypeCheckOp::IsInt as u8   => arg1.is_int(),
        v if v == TypeCheckOp::IsFloat as u8 => arg1.is_f64(),
        v if v == TypeCheckOp::IsBool as u8  => arg1.is_bool(),
        v if v == TypeCheckOp::IsStr as u8   => arg1.is_sso() || (arg1.is_heap()),
        v if v == TypeCheckOp::IsNull as u8  => arg1.is_null(),
        _ => false,
    };
    Ok(VmValue::from_bool(result))
}
