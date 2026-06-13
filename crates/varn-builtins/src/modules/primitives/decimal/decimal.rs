use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, MathematicalOps};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, NativeFnResult, Value, VmValue};

/// Native implementation backing the `decimal` contract
/// (`src/modules/primitives/decimal/decimal.vn`).
pub struct Dec;

fn get_decimal(ctx: &dyn NativeCtx, this: VmValue) -> Option<Decimal> {
    match ctx.extract(this) {
        Value::Decimal(d) => Some(*d),
        Value::Int(n) => Some(Decimal::from(n)),
        _ => None,
    }
}

fn alloc_decimal(ctx: &mut dyn NativeCtx, d: Decimal) -> VmValue {
    ctx.intern(Value::Decimal(Box::new(d)))
}

varn_contract! {
    module: "globals",
    class: "decimal",
    contract: "src/modules/primitives/decimal/decimal.vn",
    impl Dec {
        fn parse(ctx: &mut dyn NativeCtx, s: &str) -> VmValue {
            match s.parse::<Decimal>() {
                Ok(d) => alloc_decimal(ctx, d),
                Err(_) => VmValue::null(),
            }
        }

        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            get_decimal(ctx, this).map(|d| d.to_string()).unwrap_or_else(|| "0".to_string())
        }
        fn toFixed(ctx: &mut dyn NativeCtx, this: VmValue, digits: Option<i64>) -> String {
            let places = digits.unwrap_or(0).max(0) as u32;
            match get_decimal(ctx, this) {
                Some(d) => format!("{:.prec$}", d.round_dp(places), prec = places as usize),
                None => "0".to_string(),
            }
        }
        fn abs(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, d.abs()), None => VmValue::null() }
        }
        fn sign(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            match get_decimal(ctx, this) {
                Some(d) if d.is_zero() => 0,
                Some(d) if d.is_sign_negative() => -1,
                Some(_) => 1,
                None => 0,
            }
        }
        fn negate(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, -d), None => VmValue::null() }
        }
        fn ceil(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, d.ceil()), None => VmValue::null() }
        }
        fn floor(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, d.floor()), None => VmValue::null() }
        }
        fn round(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, d.round()), None => VmValue::null() }
        }
        fn trunc(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, d.trunc()), None => VmValue::null() }
        }
        fn fract(ctx: &mut dyn NativeCtx, this: VmValue) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, d.fract()), None => VmValue::null() }
        }
        fn scale(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_decimal(ctx, this).map(|d| d.scale() as i64).unwrap_or(0)
        }
        fn isZero(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_decimal(ctx, this).map(|d| d.is_zero()).unwrap_or(false)
        }
        fn isPositive(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_decimal(ctx, this).map(|d| d.is_sign_positive() && !d.is_zero()).unwrap_or(false)
        }
        fn isNegative(ctx: &mut dyn NativeCtx, this: VmValue) -> bool {
            get_decimal(ctx, this).map(|d| d.is_sign_negative()).unwrap_or(false)
        }
        fn pow(ctx: &mut dyn NativeCtx, this: VmValue, exp: i64) -> VmValue {
            match get_decimal(ctx, this) { Some(d) => alloc_decimal(ctx, d.powi(exp)), None => VmValue::null() }
        }
        fn toInt(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            get_decimal(ctx, this).and_then(|d| d.trunc().to_i64()).unwrap_or(0)
        }
        fn toFloat(ctx: &mut dyn NativeCtx, this: VmValue) -> f64 {
            get_decimal(ctx, this).and_then(|d| d.to_f64()).unwrap_or(0.0)
        }
    }
}

// Free helper re-exported from `primitives/mod.rs`; unrelated to the contract.
pub fn decimal_parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
    if let Ok(d) = s.parse::<Decimal>() {
        return Ok(ctx.intern(Value::Decimal(Box::new(d))));
    }
    Ok(VmValue::null())
}
