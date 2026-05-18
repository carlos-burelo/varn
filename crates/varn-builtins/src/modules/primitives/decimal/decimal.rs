use rust_decimal::Decimal;
#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_method, varn_module, varn_static};
use varn_types::{NativeCtx, NativeFnResult, Value, VmValue};

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

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("decimal")]
    pub mod decimal_class {
        use super::*;

        #[varn_static("parse")]
        pub fn parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
            if let Ok(d) = s.parse::<Decimal>() {
                return Ok(alloc_decimal(ctx, d));
            }
            Ok(VmValue::null())
        }

        #[varn_method("toString")]
        pub fn to_str(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(d) = get_decimal(ctx, this) {
                return Ok(ctx.alloc_str_owned(d.to_string()));
            }
            Ok(ctx.alloc_str("0"))
        }

        #[varn_method("toFixed")]
        pub fn to_fixed(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> NativeFnResult {
            let places = args
                .first()
                .and_then(|&v| {
                    if let Value::Int(n) = ctx.extract(v) {
                        Some(n as u32)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            if let Some(d) = get_decimal(ctx, this) {
                let rounded = d.round_dp(places);
                let s = format!("{:.prec$}", rounded, prec = places as usize);
                return Ok(ctx.alloc_str_owned(s));
            }
            Ok(ctx.alloc_str("0"))
        }

        #[varn_method("abs")]
        pub fn abs(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(d) = get_decimal(ctx, this) {
                return Ok(alloc_decimal(ctx, d.abs()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("negate")]
        pub fn negate(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(d) = get_decimal(ctx, this) {
                return Ok(alloc_decimal(ctx, -d));
            }
            Ok(VmValue::null())
        }

        #[varn_method("floor")]
        pub fn floor(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(d) = get_decimal(ctx, this) {
                return Ok(alloc_decimal(ctx, d.floor()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("ceil")]
        pub fn ceil(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(d) = get_decimal(ctx, this) {
                return Ok(alloc_decimal(ctx, d.ceil()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("round")]
        pub fn round(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(d) = get_decimal(ctx, this) {
                return Ok(alloc_decimal(ctx, d.round()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("trunc")]
        pub fn trunc(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(d) = get_decimal(ctx, this) {
                return Ok(alloc_decimal(ctx, d.trunc()));
            }
            Ok(VmValue::null())
        }

        #[varn_method("isZero")]
        pub fn is_zero(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_decimal(ctx, this).map(|d| d.is_zero()).unwrap_or(false),
            ))
        }

        #[varn_method("isPositive")]
        pub fn is_positive(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_decimal(ctx, this)
                    .map(|d| d.is_sign_positive() && !d.is_zero())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("isNegative")]
        pub fn is_negative(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_decimal(ctx, this)
                    .map(|d| d.is_sign_negative())
                    .unwrap_or(false),
            ))
        }
    }
}

pub fn decimal_parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
    if let Ok(d) = s.parse::<Decimal>() {
        return Ok(ctx.intern(Value::Decimal(Box::new(d))));
    }
    Ok(VmValue::null())
}
