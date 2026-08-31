use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, NativeFnResult, VmValue};

pub struct Float;

varn_contract! {
    module: "globals",
    class: "float",
    contract: "src/modules/primitives/float/float.vn",
    impl Float {

        fn MAX_VALUE(_ctx: &mut dyn NativeCtx) -> f64 { f64::MAX }
        fn MIN_VALUE(_ctx: &mut dyn NativeCtx) -> f64 { f64::MIN }
        fn EPSILON(_ctx: &mut dyn NativeCtx) -> f64 { f64::EPSILON }

        fn parse(_ctx: &mut dyn NativeCtx, s: &str) -> f64 {
            s.trim().parse::<f64>().unwrap_or(f64::NAN)
        }
        fn isNaN(_ctx: &mut dyn NativeCtx, val: VmValue) -> bool {
            val.is_f64() && val.as_f64().is_nan()
        }
        fn isFinite(_ctx: &mut dyn NativeCtx, val: VmValue) -> bool {
            if val.is_f64() { val.as_f64().is_finite() } else { val.is_int() }
        }
        fn isInfinite(_ctx: &mut dyn NativeCtx, val: VmValue) -> bool {
            val.is_f64() && val.as_f64().is_infinite()
        }


        fn toString(_ctx: &mut dyn NativeCtx, this: f64) -> String { this.to_string() }
        fn valueOf(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this }
        fn toFixed(_ctx: &mut dyn NativeCtx, this: f64, digits: Option<i64>) -> String {
            let d = digits.unwrap_or(0).max(0) as usize;
            format!("{:.*}", d, this)
        }
        fn abs(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.abs() }
        fn sign(_ctx: &mut dyn NativeCtx, this: f64) -> i64 {
            if this > 0.0 { 1 } else if this < 0.0 { -1 } else { 0 }
        }
        fn negate(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { -this }
        fn min(_ctx: &mut dyn NativeCtx, this: f64, other: f64) -> f64 { this.min(other) }
        fn max(_ctx: &mut dyn NativeCtx, this: f64, other: f64) -> f64 { this.max(other) }
        fn clamp(_ctx: &mut dyn NativeCtx, this: f64, lo: f64, hi: f64) -> f64 {
            this.max(lo).min(hi)
        }
        fn pow(_ctx: &mut dyn NativeCtx, this: f64, exponent: f64) -> f64 { this.powf(exponent) }
        fn isInteger(_ctx: &mut dyn NativeCtx, this: f64) -> bool {
            this.is_finite() && this.fract() == 0.0
        }
        fn floor(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.floor() }
        fn ceil(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.ceil() }
        fn round(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.round() }
        fn trunc(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.trunc() }
        /// Truncating float -> int. The inverse of `int.toFloat`, and the only
        /// way to land a computed value in an `int` slot: `as int` is a type
        /// assertion with no runtime conversion, so it would leave float bits
        /// in a register the backend then reads as an int payload.
        fn toInt(_ctx: &mut dyn NativeCtx, this: f64) -> i64 { this.trunc() as i64 }
    }
}

pub fn float_is_finite(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        if v.is_f64() {
            return Ok(VmValue::from_bool(v.as_f64().is_finite()));
        }
        return Ok(VmValue::from_bool(v.is_int()));
    }
    Ok(VmValue::bool_false())
}

pub fn float_is_nan(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        return Ok(VmValue::from_bool(v.is_f64() && v.as_f64().is_nan()));
    }
    Ok(VmValue::bool_false())
}

pub fn float_parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
    let f = s.trim().parse::<f64>().unwrap_or(f64::NAN);
    Ok(VmValue::from_f64(f))
}
