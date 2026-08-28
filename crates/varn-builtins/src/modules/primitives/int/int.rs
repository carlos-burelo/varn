use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, NativeFnResult, VmValue};

pub struct Int;

const INT_MAX: i64 = (1 << 47) - 1;
const INT_MIN: i64 = -(1 << 47);

varn_contract! {
    module: "globals",
    class: "int",
    contract: "src/modules/primitives/int/int.vn",
    impl Int {

        fn MAX_VALUE(_ctx: &mut dyn NativeCtx) -> i64 { INT_MAX }
        fn MIN_VALUE(_ctx: &mut dyn NativeCtx) -> i64 { INT_MIN }

        fn parse(_ctx: &mut dyn NativeCtx, s: &str) -> i64 {
            s.trim().parse::<i64>().unwrap_or(0)
        }
        fn isInteger(_ctx: &mut dyn NativeCtx, val: VmValue) -> bool {
            val.is_int()
        }
        fn isSafeInteger(_ctx: &mut dyn NativeCtx, val: i64) -> bool {
            (INT_MIN..=INT_MAX).contains(&val)
        }


        fn toString(_ctx: &mut dyn NativeCtx, this: i64) -> String { this.to_string() }
        fn valueOf(_ctx: &mut dyn NativeCtx, this: i64) -> i64 { this }
        fn toLocaleString(_ctx: &mut dyn NativeCtx, this: i64) -> String { this.to_string() }

        fn toFixed(_ctx: &mut dyn NativeCtx, this: i64, digits: Option<i64>) -> String {
            let d = digits.unwrap_or(0).max(0) as usize;
            format!("{:.*}", d, this as f64)
        }

        fn abs(_ctx: &mut dyn NativeCtx, this: i64) -> i64 { this.abs() }
        fn sign(_ctx: &mut dyn NativeCtx, this: i64) -> i64 { this.signum() }
        fn negate(_ctx: &mut dyn NativeCtx, this: i64) -> i64 { -this }
        fn bitwiseNot(_ctx: &mut dyn NativeCtx, this: i64) -> i64 { !this }
        fn min(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.min(other) }
        fn max(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.max(other) }
        fn clamp(_ctx: &mut dyn NativeCtx, this: i64, lo: i64, hi: i64) -> i64 {
            this.max(lo).min(hi)
        }

        fn toHex(_ctx: &mut dyn NativeCtx, this: i64) -> String {
            if this < 0 { format!("-{:x}", (this as i128).unsigned_abs()) } else { format!("{:x}", this) }
        }
        fn toBinary(_ctx: &mut dyn NativeCtx, this: i64) -> String {
            if this < 0 { format!("-{:b}", (this as i128).unsigned_abs()) } else { format!("{:b}", this) }
        }
        fn toOctal(_ctx: &mut dyn NativeCtx, this: i64) -> String {
            if this < 0 { format!("-{:o}", (this as i128).unsigned_abs()) } else { format!("{:o}", this) }
        }

        fn toFloat(_ctx: &mut dyn NativeCtx, this: i64) -> f64 { this as f64 }
        fn pow(_ctx: &mut dyn NativeCtx, this: i64, exponent: i64) -> i64 {
            this.wrapping_pow(exponent.max(0) as u32)
        }
        fn isEven(_ctx: &mut dyn NativeCtx, this: i64) -> bool { this % 2 == 0 }
        fn isOdd(_ctx: &mut dyn NativeCtx, this: i64) -> bool { this % 2 != 0 }
    }
}

pub fn int_is_integer(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        return Ok(VmValue::from_bool(v.is_int()));
    }
    Ok(VmValue::bool_false())
}

pub fn int_parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        if let Some(s) = ctx.str_owned(v) {
            return Ok(VmValue::from_int(s.trim().parse::<i64>().unwrap_or(0)));
        }
    }
    Ok(VmValue::from_int(0))
}
