use bigdecimal::{BigDecimal, RoundingMode};
use num_traits::{Signed, ToPrimitive, Zero};
use varn_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, VmValue};

pub struct Dec;

fn get_decimal(ctx: &dyn NativeCtx, this: VmValue) -> Option<BigDecimal> {
    if ctx.is_int(this) {
        return Some(BigDecimal::from(ctx.as_int(this)));
    }
    ctx.as_decimal(this)
}

fn alloc_decimal(ctx: &mut dyn NativeCtx, d: BigDecimal) -> VmValue {
    ctx.alloc_decimal(d)
}

fn rounded(ctx: &mut dyn NativeCtx, this: VmValue, mode: RoundingMode) -> VmValue {
    match get_decimal(ctx, this) {
        Some(d) => alloc_decimal(ctx, d.with_scale_round(0, mode)),
        None => VmValue::null(),
    }
}

fn not_decimal() -> NativeError {
    NativeError::type_error("decimal: receiver is not a decimal")
}

fn pow_decimal(d: &BigDecimal, exp: i64) -> Result<BigDecimal, NativeError> {
    use varn_core::numeric_big::DecimalPowFault;
    varn_core::numeric_big::pow_decimal(d, &BigDecimal::from(exp)).map_err(|fault| match fault {
        DecimalPowFault::NonIntegral => NativeError::range_error("the exponent of a decimal power must be an integer"),
        DecimalPowFault::TooLarge => NativeError::range_error("the result of this decimal power would have too many digits"),
        DecimalPowFault::ZeroToNegative => NativeError::division_by_zero("zero cannot be raised to a negative power"),
    })
}

varn_contract! {
    module: "globals",
    class: "decimal",
    contract: "src/modules/core/types/decimal/decimal.vn",
    impl Dec {
        fn parse(ctx: &mut dyn NativeCtx, s: &str) -> Result<VmValue, NativeError> {
            let d = s
                .trim()
                .parse::<BigDecimal>()
                .map_err(|e| NativeError::std(varn_core::StdErrorClass::Parse, format!("decimal.parse({s:?}): {e}")))?;
            Ok(alloc_decimal(ctx, d))
        }
        fn tryParse(ctx: &mut dyn NativeCtx, s: &str) -> Option<VmValue> {
            let d = s.trim().parse::<BigDecimal>().ok()?;
            Some(alloc_decimal(ctx, d))
        }

        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            get_decimal(ctx, this).map(|d| d.to_plain_string()).unwrap_or_else(|| "0".to_string())
        }
        fn toFixed(ctx: &mut dyn NativeCtx, this: VmValue, digits: Option<i64>) -> Result<String, NativeError> {
            let places = varn_core::fixed_digits(digits).ok_or_else(|| {
                NativeError::range_error(format!("toFixed: digits must be within 0..={}", varn_core::FIXED_MAX_DIGITS))
            })?;
            Ok(match get_decimal(ctx, this) {
                Some(d) => d.with_scale_round(places as i64, RoundingMode::HalfEven).to_plain_string(),
                None => "0".to_string(),
            })
        }
        fn abs(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, d.abs()), None => VmValue::null() }
        }
        fn sign(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            match get_decimal(ctx, this) {
                Some(d) if d.is_zero() => 0,
                Some(d) if d.is_negative() => -1,
                Some(_) => 1,
                None => 0,
            }
        }
        fn negate(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, -d), None => VmValue::null() }
        }
        fn ceil(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue { rounded(ctx, this, RoundingMode::Ceiling) }
        fn floor(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue { rounded(ctx, this, RoundingMode::Floor) }
        fn round(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue { rounded(ctx, this, RoundingMode::HalfEven) }
        fn trunc(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue { rounded(ctx, this, RoundingMode::Down) }
        fn fract(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) {
                Some(d) => {
                    let whole = d.with_scale_round(0, RoundingMode::Down);
                    alloc_decimal(ctx, d - whole)
                }
                None => VmValue::null(),
            }
        }
        fn scale(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_decimal(ctx, this).map(|d| d.as_bigint_and_exponent().1).unwrap_or(0)
        }
        fn isZero(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_decimal(ctx, this).map(|d| d.is_zero()).unwrap_or(false)
        }
        fn isPositive(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_decimal(ctx, this).map(|d| d.is_positive()).unwrap_or(false)
        }
        fn isNegative(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_decimal(ctx, this).map(|d| d.is_negative()).unwrap_or(false)
        }
        fn pow(ctx: &mut dyn NativeCtx, this: VmValue, exp: i64) -> Result<VmValue, NativeError> {
            let d = get_decimal(ctx, this).ok_or_else(not_decimal)?;
            let r = pow_decimal(&d, exp)?;
            Ok(alloc_decimal(ctx, r))
        }
        fn toInt(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<i64, NativeError> {
            let d = get_decimal(ctx, this).ok_or_else(not_decimal)?;
            d.with_scale_round(0, RoundingMode::Down).to_i64().ok_or_else(|| {
                NativeError::integer_overflow(format!("integer overflow: {d} does not fit int"))
            })
        }
        fn toFloat(ctx: &mut dyn NativeCtx, this: VmValue) -> f64 {
            get_decimal(ctx, this).and_then(|d| d.to_f64()).unwrap_or(0.0)
        }
    }
}
