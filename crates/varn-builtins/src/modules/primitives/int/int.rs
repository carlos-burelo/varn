use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, VmValue};

pub struct Int;

// Taken from `varn_core`, not restated: these used to say `2^47` while the
// arithmetic checked `i64`, so `int.MAX_VALUE + 1` wrapped in silence instead
// of raising.
const INT_MAX: i64 = varn_core::INT_MAX;
const INT_MIN: i64 = varn_core::INT_MIN;

varn_contract! {
    module: "globals",
    class: "int",
    contract: "src/modules/primitives/int/int.vn",
    impl Int {

        fn MAX_VALUE(_ctx: &mut dyn NativeCtx) -> i64 { INT_MAX }
        fn MIN_VALUE(_ctx: &mut dyn NativeCtx) -> i64 { INT_MIN }

        fn parse(_ctx: &mut dyn NativeCtx, s: &str) -> Result<i64, NativeError> {
            s.trim()
                .parse::<i64>()
                .map_err(|e| NativeError::from(format!("int.parse({s:?}): {e}")))
        }
        fn tryParse(_ctx: &mut dyn NativeCtx, s: &str) -> Option<i64> {
            s.trim().parse::<i64>().ok()
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

        fn abs(_ctx: &mut dyn NativeCtx, this: i64) -> Result<i64, NativeError> {
            this.checked_abs().ok_or_else(|| overflow_error("abs", this))
        }
        fn sign(_ctx: &mut dyn NativeCtx, this: i64) -> i64 { this.signum() }
        fn negate(_ctx: &mut dyn NativeCtx, this: i64) -> Result<i64, NativeError> {
            varn_core::neg_int(this).ok_or_else(|| overflow_error("negate", this))
        }
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
        fn pow(_ctx: &mut dyn NativeCtx, this: i64, exponent: i64) -> Result<i64, NativeError> {
            let e = u32::try_from(exponent)
                .map_err(|_| NativeError::from(format!("pow: exponent {exponent} must be in 0..=4294967295")))?;
            varn_core::pow_int(this, e).ok_or_else(|| overflow_error("pow", this))
        }
        fn isEven(_ctx: &mut dyn NativeCtx, this: i64) -> bool { this % 2 == 0 }
        fn isOdd(_ctx: &mut dyn NativeCtx, this: i64) -> bool { this % 2 != 0 }

        fn wrappingAdd(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.wrapping_add(other) }
        fn wrappingSub(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.wrapping_sub(other) }
        fn wrappingMul(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.wrapping_mul(other) }
        fn saturatingAdd(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.saturating_add(other) }
        fn saturatingSub(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.saturating_sub(other) }
        fn saturatingMul(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> i64 { this.saturating_mul(other) }
        fn checkedAdd(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Option<i64> { varn_core::add_int(this, other) }
        fn checkedSub(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Option<i64> { varn_core::sub_int(this, other) }
        fn checkedMul(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Option<i64> { varn_core::mul_int(this, other) }
        fn div(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::div_int(this, other).map_err(|f| div_fault(f, "div", this, other))
        }
        fn floorDiv(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::floor_div_int(this, other).map_err(|f| div_fault(f, "floorDiv", this, other))
        }
        fn ceilDiv(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::ceil_div_int(this, other).map_err(|f| div_fault(f, "ceilDiv", this, other))
        }
        fn rem(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::rem_int(this, other).map_err(|f| div_fault(f, "rem", this, other))
        }
        fn r#mod(_ctx: &mut dyn NativeCtx, this: i64, other: i64) -> Result<i64, NativeError> {
            varn_core::mod_int(this, other).map_err(|f| div_fault(f, "mod", this, other))
        }
    }
}

fn overflow_error(op: &str, v: i64) -> NativeError {
    NativeError::integer_overflow(format!(
        "integer overflow: {op}({v}) is outside int ({INT_MIN}..={INT_MAX})"
    ))
}

fn div_fault(fault: varn_core::IntDivFault, op: &str, a: i64, b: i64) -> NativeError {
    match fault {
        varn_core::IntDivFault::DivisionByZero => {
            NativeError::division_by_zero(format!("{op}: division by zero"))
        }
        varn_core::IntDivFault::Overflow => {
            NativeError::integer_overflow(format!("integer overflow: {a}.{op}({b}) is outside int"))
        }
    }
}
