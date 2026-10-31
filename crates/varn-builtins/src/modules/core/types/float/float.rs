use varn_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, NativeFnResult, VmValue};

pub struct Float;

varn_contract! {
    module: "globals",
    class: "float",
    contract: "src/modules/core/types/float/float.vn",
    impl Float {

        fn MAX_VALUE(_ctx: &mut dyn NativeCtx) -> f64 { f64::MAX }
        fn LOWEST(_ctx: &mut dyn NativeCtx) -> f64 { f64::MIN }
        fn MIN_POSITIVE(_ctx: &mut dyn NativeCtx) -> f64 { f64::MIN_POSITIVE }
        fn NAN(_ctx: &mut dyn NativeCtx) -> f64 { f64::NAN }
        fn INFINITY(_ctx: &mut dyn NativeCtx) -> f64 { f64::INFINITY }
        fn NEG_INFINITY(_ctx: &mut dyn NativeCtx) -> f64 { f64::NEG_INFINITY }
        fn EPSILON(_ctx: &mut dyn NativeCtx) -> f64 { f64::EPSILON }

        fn parse(_ctx: &mut dyn NativeCtx, s: &str) -> Result<f64, NativeError> {
            s.trim()
                .parse::<f64>()
                .map_err(|e| NativeError::std(varn_core::StdErrorClass::Parse, format!("float.parse({s:?}): {e}")))
        }
        fn tryParse(_ctx: &mut dyn NativeCtx, s: &str) -> Option<f64> {
            s.trim().parse::<f64>().ok()
        }
        fn isNaN(_ctx: &mut dyn NativeCtx, this: f64) -> bool { this.is_nan() }
        fn isFinite(_ctx: &mut dyn NativeCtx, this: f64) -> bool { this.is_finite() }
        fn isInfinite(_ctx: &mut dyn NativeCtx, this: f64) -> bool { this.is_infinite() }


        fn toString(_ctx: &mut dyn NativeCtx, this: f64) -> String { varn_core::float_to_string(this) }
        fn valueOf(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this }
        fn toFixed(_ctx: &mut dyn NativeCtx, this: f64, digits: Option<i64>) -> Result<String, NativeError> {
            let d = varn_core::fixed_digits(digits).ok_or_else(|| {
                NativeError::range_error(format!("toFixed: digits must be within 0..={}", varn_core::FIXED_MAX_DIGITS))
            })?;
            Ok(varn_core::float_to_fixed(this, d))
        }
        fn abs(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.abs() }
        fn sign(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { varn_core::fsign(this) }
        fn negate(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { -this }
        fn min(_ctx: &mut dyn NativeCtx, this: f64, other: f64) -> f64 { varn_core::fmin(this, other) }
        fn max(_ctx: &mut dyn NativeCtx, this: f64, other: f64) -> f64 { varn_core::fmax(this, other) }
        fn clamp(_ctx: &mut dyn NativeCtx, this: f64, lo: f64, hi: f64) -> Result<f64, NativeError> {
            if lo > hi {
                return Err(NativeError::range_error(format!("clamp: the lower bound {lo} is above the upper bound {hi}")));
            }
            Ok(varn_core::fmin(varn_core::fmax(this, lo), hi))
        }
        fn pow(_ctx: &mut dyn NativeCtx, this: f64, exponent: f64) -> f64 { this.powf(exponent) }
        fn isInteger(_ctx: &mut dyn NativeCtx, this: f64) -> bool {
            this.is_finite() && this.fract() == 0.0
        }
        fn floor(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.floor() }
        fn ceil(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.ceil() }
        fn round(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.round() }
        fn trunc(_ctx: &mut dyn NativeCtx, this: f64) -> f64 { this.trunc() }




        fn toInt(_ctx: &mut dyn NativeCtx, this: f64) -> Result<i64, NativeError> {
            varn_core::float_to_int(this)
                .ok_or_else(|| NativeError::integer_overflow(format!("integer overflow: {this} does not fit int")))
        }
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
