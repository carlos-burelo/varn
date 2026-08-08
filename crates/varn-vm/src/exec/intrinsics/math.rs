use crate::error::VmResult;
use crate::value::VmValue;
use varn_core::intrinsic_ops::math::MathOp;

pub(crate) fn dispatch(op: u8, args: &[VmValue]) -> VmResult<VmValue> {
    let Some(arg1) = args.get(1) else {
        return Ok(VmValue::null());
    };
    let x = arg1.to_f64();
    let result = match op {
        v if v == MathOp::Abs as u8 => x.abs(),
        v if v == MathOp::Sqrt as u8 => x.sqrt(),
        v if v == MathOp::Floor as u8 => x.floor(),
        v if v == MathOp::Ceil as u8 => x.ceil(),
        v if v == MathOp::Round as u8 => x.round(),
        v if v == MathOp::Sin as u8 => x.sin(),
        v if v == MathOp::Cos as u8 => x.cos(),
        v if v == MathOp::Tan as u8 => x.tan(),
        v if v == MathOp::Log as u8 => x.ln(),
        v if v == MathOp::Exp as u8 => x.exp(),
        v if v == MathOp::Pow as u8 => {
            let y = args.get(2).map(|v| v.to_f64()).unwrap_or(1.0);
            x.powf(y)
        }
        v if v == MathOp::Min as u8 => args[2..].iter().fold(x, |acc, v| acc.min(v.to_f64())),
        v if v == MathOp::Max as u8 => args[2..].iter().fold(x, |acc, v| acc.max(v.to_f64())),
        _ => return Ok(VmValue::null()),
    };
    if arg1.is_int()
        && result.fract() == 0.0
        && result >= i64::MIN as f64
        && result <= i64::MAX as f64
    {
        Ok(VmValue::from_int(result as i64))
    } else {
        Ok(VmValue::from_f64(result))
    }
}
