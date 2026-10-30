use crate::error::{RuntimeError, VmResult};
use crate::value::VmValue;
use varn_core::intrinsic_ops::math::MathOp;
use varn_core::{fmax, fmin};

pub(crate) fn dispatch(op: u8, args: &[VmValue]) -> VmResult<VmValue> {
    let op = MathOp::of_wire(op)
        .ok_or_else(|| RuntimeError::new(format!("unknown math intrinsic {op:#x}")))?;
    let Some(arg1) = args.get(1) else {
        return Err(RuntimeError::type_error(format!(
            "{} needs an operand",
            op.name()
        )));
    };
    let x = arg1.to_f64();
    let rest = args.get(2..).unwrap_or(&[]);
    let result = match op {
        MathOp::Abs => x.abs(),
        MathOp::Sqrt => x.sqrt(),
        MathOp::Floor => x.floor(),
        MathOp::Ceil => x.ceil(),
        MathOp::Round => x.round(),
        MathOp::Sin => x.sin(),
        MathOp::Cos => x.cos(),
        MathOp::Tan => x.tan(),
        MathOp::Log => x.ln(),
        MathOp::Exp => x.exp(),
        MathOp::Pow => x.powf(rest.first().map_or(1.0, |v| v.to_f64())),
        MathOp::Min => rest.iter().fold(x, |acc, v| fmin(acc, v.to_f64())),
        MathOp::Max => rest.iter().fold(x, |acc, v| fmax(acc, v.to_f64())),
    };
    Ok(VmValue::from_f64(result))
}
