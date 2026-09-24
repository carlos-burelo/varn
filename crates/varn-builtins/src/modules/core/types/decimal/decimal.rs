use bigdecimal::{BigDecimal, RoundingMode};
use num_traits::{One, Signed, ToPrimitive, Zero};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, Value, VmValue};

pub struct Dec;

fn get_decimal(ctx: &dyn NativeCtx, this: VmValue) -> Option<BigDecimal> {
    match ctx.extract(this) {
        Value::Decimal(d) => Some(*d),
        Value::Int(n) => Some(BigDecimal::from(n)),
        _ => None,
    }
}

fn alloc_decimal(ctx: &mut dyn NativeCtx, d: BigDecimal) -> VmValue {
    ctx.intern(Value::Decimal(Box::new(d)))
}

fn rounded(ctx: &mut dyn NativeCtx, this: VmValue, mode: RoundingMode) -> VmValue {
    match get_decimal(ctx, this) {
        Some(d) => alloc_decimal(ctx, d.with_scale_round(0, mode)),
        None => VmValue::null(),
    }
}

fn not_decimal() -> NativeError {
    NativeError::from("decimal: receiver is not a decimal")
}

/// `d^exp`, exact for `exp >= 0`; a negative exponent divides with the
/// `decimal` quotient rule.
fn pow_decimal(d: &BigDecimal, exp: i64) -> Result<BigDecimal, NativeError> {
    let mut result = BigDecimal::one();
    let mut base = d.clone();
    let mut e = exp.unsigned_abs();
    while e > 0 {
        if e & 1 == 1 {
            result = &result * &base;
        }
        base = &base * &base;
        e >>= 1;
    }
    if exp >= 0 {
        return Ok(result);
    }
    varn_core::numeric_big::div_decimal(&BigDecimal::one(), &result)
        .map_err(|_| NativeError::division_by_zero("decimal.pow: zero to a negative power"))
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
                .map_err(|e| NativeError::from(format!("decimal.parse({s:?}): {e}")))?;
            Ok(alloc_decimal(ctx, d))
        }
        fn tryParse(ctx: &mut dyn NativeCtx, s: &str) -> Option<VmValue> {
            let d = s.trim().parse::<BigDecimal>().ok()?;
            Some(alloc_decimal(ctx, d))
        }

        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            get_decimal(ctx, this).map(|d| d.to_plain_string()).unwrap_or_else(|| "0".to_string())
        }
        fn toFixed(ctx: &mut dyn NativeCtx, this: VmValue, digits: Option<i64>) -> String {
            let places = digits.unwrap_or(0).max(0);
            match get_decimal(ctx, this) {
                Some(d) => d.with_scale_round(places, RoundingMode::HalfEven).to_plain_string(),
                None => "0".to_string(),
            }
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
